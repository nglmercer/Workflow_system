use std::collections::HashMap;
use std::sync::Arc;

use flume::Receiver;
use workflow_domain::{
    ActionOrGroup, EngineEvent, EngineEventPayload, RuleEngineConfig, TriggerContext,
    TriggerResult, TriggerRule, WorkflowResult,
};

use crate::actions::ActionHandler;
use crate::conditions::evaluate_condition;
use crate::cooldown::CooldownTracker;
use crate::emitter::EventEmitter;

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

    async fn execute_action(
        &self,
        action: &workflow_domain::Action,
        context: &TriggerContext,
    ) -> workflow_domain::ExecutedAction {
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

        #[cfg(feature = "delay")]
        if let Some(delay) = action.delay {
            tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
        }

        match self.handlers.get(&action.action_type) {
            Some(handler) => {
                self.emitter.emit(EngineEventPayload {
                    event: EngineEvent::ActionSuccess,
                    rule: None,
                    context: Some(context.clone()),
                    action_type: Some(action.action_type.clone()),
                    result: None,
                    error: None,
                });

                match handler.execute(&action.params, context).await {
                    Ok(result) => workflow_domain::ExecutedAction {
                        action_type: action.action_type.clone(),
                        result: Some(result),
                        error: None,
                        timestamp: chrono::Utc::now().timestamp_millis(),
                        skipped: None,
                    },
                    Err(e) => {
                        self.emitter.emit(EngineEventPayload {
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
                if self.settings.strict_actions {
                    self.emitter.emit(EngineEventPayload {
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
                    self.emitter.emit(EngineEventPayload {
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
