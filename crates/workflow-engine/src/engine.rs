use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;

use flume::Receiver;
use workflow_domain::{
    ActionOrGroup, EngineEvent, EngineEventPayload, RepeatCount, RuleEngineConfig, TriggerContext,
    TriggerResult, TriggerRule, WorkflowResult,
};

use crate::actions::ActionHandler;
use crate::conditions::evaluate_condition;
use crate::cooldown::CooldownTracker;
use crate::emitter::EventEmitter;
use crate::expressions::{build_context_with_vars, evaluate_expressions, evaluate_field};

type BoxFuture<T> = Pin<Box<dyn std::future::Future<Output = T> + Send>>;

pub struct RuleEngine {
    rules: Vec<TriggerRule>,
    settings: workflow_domain::GlobalSettings,
    handlers: HashMap<String, Arc<dyn ActionHandler>>,
    cooldowns: CooldownTracker,
    emitter: EventEmitter,
}

impl RuleEngine {
    pub fn new(config: RuleEngineConfig) -> Self {
        let mut rules = config.rules;
        rules.sort_by_key(|b| std::cmp::Reverse(b.priority_value()));

        Self {
            rules,
            settings: config.global_settings,
            handlers: HashMap::new(),
            cooldowns: CooldownTracker::new(),
            emitter: EventEmitter::new(),
        }
    }

    pub fn register_handler(&mut self, handler: Box<dyn ActionHandler>) {
        self.handlers
            .insert(handler.action_type().to_string(), Arc::from(handler));
    }

    pub fn update_rules(&mut self, new_rules: Vec<TriggerRule>) {
        self.rules = new_rules;
        self.rules
            .sort_by_key(|b| std::cmp::Reverse(b.priority_value()));
    }

    pub fn get_rules(&self) -> &[TriggerRule] {
        &self.rules
    }

    pub fn event_receiver(&self) -> Receiver<EngineEventPayload> {
        self.emitter.subscribe()
    }

    pub async fn process_event_simple(
        &mut self,
        event_type: &str,
        data: serde_json::Value,
        vars: Option<serde_json::Value>,
    ) -> WorkflowResult<Vec<TriggerResult>> {
        let mut ctx = TriggerContext::new(event_type, data);
        if let Some(v) = vars {
            ctx.vars = Some(v);
        }
        self.evaluate_context(ctx).await
    }

    /// Process a list of emitted event names through the engine.
    /// Each event is dispatched with the given data payload. This
    /// bridges the `.flow` evaluator's `emit()` calls to the
    /// engine's rule matching, allowing workflows to trigger
    /// downstream rules via emitted events.
    pub async fn process_emitted_events(
        &mut self,
        emitted: &[String],
        data: &serde_json::Value,
    ) -> WorkflowResult<Vec<TriggerResult>> {
        let mut all_results = Vec::new();
        for event_name in emitted {
            let results = self
                .process_event_simple(event_name, data.clone(), None)
                .await?;
            all_results.extend(results);
        }
        Ok(all_results)
    }

    pub async fn process_event(
        &mut self,
        event: workflow_domain::TriggerEvent,
        vars: Option<serde_json::Value>,
    ) -> WorkflowResult<Vec<TriggerResult>> {
        let mut ctx = TriggerContext::new(event.event, event.data);
        ctx.id = event.id;
        if let Some(v) = vars {
            ctx.vars = Some(v);
        }
        self.evaluate_context(ctx).await
    }

    pub async fn evaluate_context(
        &mut self,
        context: TriggerContext,
    ) -> WorkflowResult<Vec<TriggerResult>> {
        self.emitter.emit(EngineEventPayload {
            event: EngineEvent::EngineStart,
            rule: None,
            context: Some(context.clone()),
            action_type: None,
            result: None,
            error: None,
        });

        let mut results = Vec::new();

        for rule in &self.rules {
            if !rule.is_enabled() {
                continue;
            }

            if rule.on != context.event {
                continue;
            }

            if let Some(cooldown_ms) = rule.metadata.cooldown {
                if self
                    .cooldowns
                    .is_in_cooldown(&rule.metadata.id, cooldown_ms)
                {
                    self.emitter.emit(EngineEventPayload {
                        event: EngineEvent::RuleSkip,
                        rule: Some(rule.clone()),
                        context: Some(context.clone()),
                        action_type: None,
                        result: None,
                        error: Some("Cooldown active".to_string()),
                    });
                    continue;
                }
            }

            let matched = match &rule.condition {
                Some(condition) => evaluate_condition(condition, &context)?,
                None => true,
            };

            if !matched {
                continue;
            }

            self.emitter.emit(EngineEventPayload {
                event: EngineEvent::RuleMatch,
                rule: Some(rule.clone()),
                context: Some(context.clone()),
                action_type: None,
                result: None,
                error: None,
            });

            let result = self.execute_rule(rule, &context).await;

            if rule.metadata.cooldown.is_some() {
                self.cooldowns.record_execution(&rule.metadata.id);
            }

            results.push(result);

            if !self.settings.evaluate_all {
                break;
            }
        }

        self.emitter.emit(EngineEventPayload {
            event: EngineEvent::EngineDone,
            rule: None,
            context: Some(context),
            action_type: None,
            result: None,
            error: None,
        });

        Ok(results)
    }

    async fn execute_rule(&self, rule: &TriggerRule, context: &TriggerContext) -> TriggerResult {
        match &rule.r#do {
            ActionOrGroup::Single(action) => {
                let executed = self.execute_action(action, context).await;
                TriggerResult::success(&rule.metadata.id, vec![executed])
            }
            ActionOrGroup::Multiple(actions) => {
                let mut executed_actions = Vec::new();
                for action in actions {
                    executed_actions.push(self.execute_action(action, context).await);
                }
                TriggerResult::success(&rule.metadata.id, executed_actions)
            }
            ActionOrGroup::Group(group) => {
                let mut executed_actions = Vec::new();
                match group.mode {
                    workflow_domain::ActionGroupMode::All
                    | workflow_domain::ActionGroupMode::Sequence => {
                        for action in &group.actions {
                            executed_actions.push(self.execute_action(action, context).await);
                        }
                    }
                    workflow_domain::ActionGroupMode::Either => {
                        for action in &group.actions {
                            let result = self.execute_action(action, context).await;
                            if result.error.is_none() {
                                executed_actions.push(result);
                                break;
                            }
                            executed_actions.push(result);
                        }
                    }
                }
                TriggerResult::success(&rule.metadata.id, executed_actions)
            }
        }
    }

    fn execute_action(
        &self,
        action: &workflow_domain::Action,
        context: &TriggerContext,
    ) -> BoxFuture<workflow_domain::ExecutedAction> {
        let action = action.clone();
        let context = context.clone();
        let handlers = self.handlers.clone();
        let settings = self.settings.clone();
        let emitter = self.emitter.clone();

        Box::pin(async move {
            // Handle probability
            if let Some(probability) = action.probability {
                if probability < 1.0 {
                    let random_val: f64 = rand_f64();
                    if random_val > probability {
                        return workflow_domain::ExecutedAction {
                            action_type: action.action_type.clone(),
                            result: None,
                            error: None,
                            timestamp: chrono::Utc::now().timestamp_millis(),
                            skipped: Some(format!(
                                "Probability {} did not pass (random: {})",
                                probability, random_val
                            )),
                        };
                    }
                }
            }

            // Handle delay
            #[cfg(feature = "delay")]
            if let Some(delay) = action.delay {
                tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
            }

            // Handle foreach loop
            if let Some(ref foreach) = action.foreach {
                return execute_foreach(foreach, &context, &handlers, &settings, &emitter).await;
            }

            // Handle while loop
            if let Some(ref while_config) = action.r#while {
                return execute_while(while_config, &context, &handlers, &settings, &emitter).await;
            }

            // Handle repeat loop
            if let Some(ref repeat) = action.repeat {
                return execute_repeat(repeat, &context, &handlers, &settings, &emitter).await;
            }

            // Execute single action with optional retry
            execute_action_with_retry(&action, &context, &handlers, &settings, &emitter).await
        })
    }
}

async fn execute_action_with_retry(
    action: &workflow_domain::Action,
    context: &TriggerContext,
    handlers: &HashMap<String, Arc<dyn ActionHandler>>,
    settings: &workflow_domain::GlobalSettings,
    emitter: &EventEmitter,
) -> workflow_domain::ExecutedAction {
    let retry_policy = action.retry.as_ref();
    let max_attempts = retry_policy.map_or(1, |p| p.max_attempts + 1);

    let mut last_error = None;

    for attempt in 0..max_attempts {
        if attempt > 0 {
            // Calculate delay with exponential backoff
            if let Some(policy) = retry_policy {
                let _delay = calculate_retry_delay(policy, attempt - 1);
                #[cfg(feature = "delay")]
                tokio::time::sleep(std::time::Duration::from_millis(_delay)).await;
            }
        }

        let result = execute_single_action(action, context, handlers, settings, emitter).await;

        if result.error.is_none() {
            return result;
        }

        last_error = result.error.clone();

        // Check if we should retry
        if let Some(policy) = retry_policy {
            if let Some(ref retry_on) = policy.retry_on {
                if let Some(ref err) = last_error {
                    let should_retry = retry_on.iter().any(|pattern| err.contains(pattern));
                    if !should_retry {
                        return result;
                    }
                }
            }
        }
    }

    // All attempts failed
    workflow_domain::ExecutedAction {
        action_type: action.action_type.clone(),
        result: None,
        error: last_error.or_else(|| Some("All retry attempts failed".to_string())),
        timestamp: chrono::Utc::now().timestamp_millis(),
        skipped: None,
    }
}

async fn execute_single_action(
    action: &workflow_domain::Action,
    context: &TriggerContext,
    handlers: &HashMap<String, Arc<dyn ActionHandler>>,
    settings: &workflow_domain::GlobalSettings,
    emitter: &EventEmitter,
) -> workflow_domain::ExecutedAction {
    match handlers.get(&action.action_type) {
        Some(handler) => {
            emitter.emit(EngineEventPayload {
                event: EngineEvent::ActionSuccess,
                rule: None,
                context: Some(context.clone()),
                action_type: Some(action.action_type.clone()),
                result: None,
                error: None,
            });

            // Evaluate expressions in params
            let params = action.params.as_ref().map(|p| {
                let mut evaluated = HashMap::new();
                let ctx_val = serde_json::to_value(context).unwrap_or_default();
                for (k, v) in p {
                    if let Ok(val) = evaluate_field(v, &ctx_val) {
                        evaluated.insert(k.clone(), val);
                    } else {
                        evaluated.insert(k.clone(), v.clone());
                    }
                }
                evaluated
            });

            match handler.execute(&params, context).await {
                Ok(result) => workflow_domain::ExecutedAction {
                    action_type: action.action_type.clone(),
                    result: Some(result),
                    error: None,
                    timestamp: chrono::Utc::now().timestamp_millis(),
                    skipped: None,
                },
                Err(e) => {
                    emitter.emit(EngineEventPayload {
                        event: EngineEvent::ActionError,
                        rule: None,
                        context: Some(context.clone()),
                        action_type: Some(action.action_type.clone()),
                        result: None,
                        error: Some(e.to_string()),
                    });

                    workflow_domain::ExecutedAction {
                        action_type: action.action_type.clone(),
                        result: None,
                        error: Some(e.to_string()),
                        timestamp: chrono::Utc::now().timestamp_millis(),
                        skipped: None,
                    }
                }
            }
        }
        None => {
            if settings.strict_actions {
                emitter.emit(EngineEventPayload {
                    event: EngineEvent::ActionError,
                    rule: None,
                    context: Some(context.clone()),
                    action_type: Some(action.action_type.clone()),
                    result: None,
                    error: Some(format!("Unknown action type: {}", action.action_type)),
                });

                workflow_domain::ExecutedAction {
                    action_type: action.action_type.clone(),
                    result: None,
                    error: Some(format!("Unknown action type: {}", action.action_type)),
                    timestamp: chrono::Utc::now().timestamp_millis(),
                    skipped: None,
                }
            } else {
                emitter.emit(EngineEventPayload {
                    event: EngineEvent::ActionSkip,
                    rule: None,
                    context: Some(context.clone()),
                    action_type: Some(action.action_type.clone()),
                    result: None,
                    error: Some(format!(
                        "No handler for action type: {}",
                        action.action_type
                    )),
                });

                workflow_domain::ExecutedAction {
                    action_type: action.action_type.clone(),
                    result: None,
                    error: None,
                    timestamp: chrono::Utc::now().timestamp_millis(),
                    skipped: Some(format!("No handler for: {}", action.action_type)),
                }
            }
        }
    }
}

async fn execute_foreach(
    foreach: &workflow_domain::ForeachConfig,
    context: &TriggerContext,
    handlers: &HashMap<String, Arc<dyn ActionHandler>>,
    settings: &workflow_domain::GlobalSettings,
    emitter: &EventEmitter,
) -> workflow_domain::ExecutedAction {
    let context_value = serde_json::to_value(context).unwrap_or_default();
    let items = match resolve_path(&foreach.field, &context_value) {
        serde_json::Value::Array(arr) => arr,
        _ => {
            return workflow_domain::ExecutedAction {
                action_type: "foreach".to_string(),
                result: None,
                error: Some(format!("Field '{}' is not an array", foreach.field)),
                timestamp: chrono::Utc::now().timestamp_millis(),
                skipped: None,
            };
        }
    };

    let _max_parallel = foreach.parallel.unwrap_or(1);
    let mut results = Vec::new();

    for (index, item) in items.iter().enumerate() {
        let mut vars = Vec::new();
        vars.push((foreach.item_var.clone(), item.clone()));
        if let Some(ref index_var) = foreach.index_var {
            vars.push((index_var.clone(), serde_json::json!(index)));
        }

        let loop_context = build_context_with_vars(&context_value, &vars);
        let loop_ctx = TriggerContext {
            event: context.event.clone(),
            timestamp: context.timestamp,
            data: loop_context,
            vars: context.vars.clone(),
            id: context.id.clone(),
        };

        for action in &foreach.actions {
            let engine = RuleEngine {
                rules: vec![],
                settings: settings.clone(),
                handlers: handlers.clone(),
                cooldowns: CooldownTracker::new(),
                emitter: emitter.clone(),
            };
            let result = engine.execute_action(action, &loop_ctx).await;
            results.push(result);
        }
    }

    workflow_domain::ExecutedAction {
        action_type: "foreach".to_string(),
        result: Some(serde_json::json!({
            "iterations": items.len(),
            "results": results.len()
        })),
        error: None,
        timestamp: chrono::Utc::now().timestamp_millis(),
        skipped: None,
    }
}

async fn execute_while(
    while_config: &workflow_domain::WhileConfig,
    context: &TriggerContext,
    handlers: &HashMap<String, Arc<dyn ActionHandler>>,
    settings: &workflow_domain::GlobalSettings,
    emitter: &EventEmitter,
) -> workflow_domain::ExecutedAction {
    let max_iterations = while_config.max_iterations.unwrap_or(1000);
    let mut iterations = 0;
    let mut results = Vec::new();

    while iterations < max_iterations {
        let should_continue = match evaluate_condition(&while_config.condition, context) {
            Ok(v) => v,
            Err(_) => break,
        };
        if !should_continue {
            break;
        }

        if let Some(_delay_ms) = while_config.delay_ms {
            #[cfg(feature = "delay")]
            tokio::time::sleep(std::time::Duration::from_millis(_delay_ms)).await;
        }

        for action in &while_config.actions {
            let engine = RuleEngine {
                rules: vec![],
                settings: settings.clone(),
                handlers: handlers.clone(),
                cooldowns: CooldownTracker::new(),
                emitter: emitter.clone(),
            };
            let result = engine.execute_action(action, context).await;
            results.push(result);
        }

        iterations += 1;
    }

    workflow_domain::ExecutedAction {
        action_type: "while".to_string(),
        result: Some(serde_json::json!({
            "iterations": iterations,
            "completed": iterations < max_iterations
        })),
        error: if iterations >= max_iterations {
            Some("Max iterations reached".to_string())
        } else {
            None
        },
        timestamp: chrono::Utc::now().timestamp_millis(),
        skipped: None,
    }
}

async fn execute_repeat(
    repeat: &workflow_domain::RepeatConfig,
    context: &TriggerContext,
    handlers: &HashMap<String, Arc<dyn ActionHandler>>,
    settings: &workflow_domain::GlobalSettings,
    emitter: &EventEmitter,
) -> workflow_domain::ExecutedAction {
    let count = match &repeat.count {
        RepeatCount::Fixed(n) => *n,
        RepeatCount::Expression(expr) => {
            let context_value = serde_json::to_value(context).unwrap_or_default();
            match evaluate_expressions(expr, &context_value) {
                Ok(val) => val.parse::<u32>().unwrap_or(0),
                Err(_) => 0,
            }
        }
    };

    let context_value = serde_json::to_value(context).unwrap_or_default();
    let mut results = Vec::new();

    for i in 0..count {
        if let Some(_delay_ms) = repeat.delay_ms {
            #[cfg(feature = "delay")]
            tokio::time::sleep(std::time::Duration::from_millis(_delay_ms)).await;
        }

        let mut vars = Vec::new();
        if let Some(ref index_var) = repeat.index_var {
            vars.push((index_var.clone(), serde_json::json!(i)));
        }

        let loop_context = build_context_with_vars(&context_value, &vars);
        let loop_ctx = TriggerContext {
            event: context.event.clone(),
            timestamp: context.timestamp,
            data: loop_context,
            vars: context.vars.clone(),
            id: context.id.clone(),
        };

        for action in &repeat.actions {
            let engine = RuleEngine {
                rules: vec![],
                settings: settings.clone(),
                handlers: handlers.clone(),
                cooldowns: CooldownTracker::new(),
                emitter: emitter.clone(),
            };
            let result = engine.execute_action(action, &loop_ctx).await;
            results.push(result);
        }
    }

    workflow_domain::ExecutedAction {
        action_type: "repeat".to_string(),
        result: Some(serde_json::json!({
            "count": count,
            "results": results.len()
        })),
        error: None,
        timestamp: chrono::Utc::now().timestamp_millis(),
        skipped: None,
    }
}

fn resolve_path(path: &str, context: &serde_json::Value) -> serde_json::Value {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = context;

    for part in &parts {
        current = match current {
            serde_json::Value::Object(map) => map.get(*part).unwrap_or(&serde_json::Value::Null),
            serde_json::Value::Array(arr) => {
                if let Ok(idx) = part.parse::<usize>() {
                    arr.get(idx).unwrap_or(&serde_json::Value::Null)
                } else {
                    &serde_json::Value::Null
                }
            }
            _ => &serde_json::Value::Null,
        };
    }

    current.clone()
}

fn calculate_retry_delay(policy: &workflow_domain::RetryPolicy, attempt: u32) -> u64 {
    let delay = policy.initial_delay_ms as f64 * policy.backoff_multiplier.powi(attempt as i32);
    (delay as u64).min(policy.max_delay_ms)
}

fn rand_f64() -> f64 {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let s = RandomState::new();
    let mut hasher = s.build_hasher();
    hasher.write_u64(chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0) as u64);
    let bits = hasher.finish();
    (bits as f64) / (u64::MAX as f64)
}
