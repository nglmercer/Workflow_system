use wasm_bindgen::prelude::*;

use workflow_actions::builtin_handlers;
use workflow_domain::{RuleEngineConfig, TriggerContext};
use workflow_engine::RuleEngine;
use workflow_parser::{FlowEvaluator, FlowParser};
use workflow_serialize::RuleExporter;

#[wasm_bindgen]
pub struct WasmRuleEngine {
    engine: RuleEngine,
}

#[wasm_bindgen]
impl WasmRuleEngine {
    #[wasm_bindgen(constructor)]
    pub fn new(config_json: &str) -> Result<WasmRuleEngine, JsError> {
        let config: RuleEngineConfig = serde_json::from_str(config_json)
            .map_err(|e| JsError::new(&format!("Invalid config JSON: {}", e)))?;

        let mut engine = RuleEngine::new(config);

        for handler in builtin_handlers() {
            engine.register_handler(handler);
        }

        Ok(Self { engine })
    }

    #[wasm_bindgen(js_name = processEventSimple)]
    pub async fn process_event_simple(
        &mut self,
        event_type: &str,
        data_json: &str,
        vars_json: Option<String>,
    ) -> Result<JsValue, JsError> {
        let data: serde_json::Value = serde_json::from_str(data_json)
            .map_err(|e| JsError::new(&format!("Invalid data JSON: {}", e)))?;

        let vars: Option<serde_json::Value> = match vars_json {
            Some(v) => Some(
                serde_json::from_str(&v)
                    .map_err(|e| JsError::new(&format!("Invalid vars JSON: {}", e)))?,
            ),
            None => None,
        };

        let results = self
            .engine
            .process_event_simple(event_type, data, vars)
            .await
            .map_err(|e| JsError::new(&e.to_string()))?;

        serde_wasm_bindgen::to_value(&results)
            .map_err(|e| JsError::new(&format!("Serialization error: {}", e)))
    }

    #[wasm_bindgen(js_name = processEvent)]
    pub async fn process_event(
        &mut self,
        event_json: &str,
        vars_json: Option<String>,
    ) -> Result<JsValue, JsError> {
        let event: workflow_domain::TriggerEvent = serde_json::from_str(event_json)
            .map_err(|e| JsError::new(&format!("Invalid event JSON: {}", e)))?;

        let vars: Option<serde_json::Value> = match vars_json {
            Some(v) => Some(
                serde_json::from_str(&v)
                    .map_err(|e| JsError::new(&format!("Invalid vars JSON: {}", e)))?,
            ),
            None => None,
        };

        let results = self
            .engine
            .process_event(event, vars)
            .await
            .map_err(|e| JsError::new(&e.to_string()))?;

        serde_wasm_bindgen::to_value(&results)
            .map_err(|e| JsError::new(&format!("Serialization error: {}", e)))
    }

    #[wasm_bindgen(js_name = updateRules)]
    pub fn update_rules(&mut self, rules_json: &str) -> Result<(), JsError> {
        let rules: Vec<workflow_domain::TriggerRule> = serde_json::from_str(rules_json)
            .map_err(|e| JsError::new(&format!("Invalid rules JSON: {}", e)))?;

        self.engine.update_rules(rules);
        Ok(())
    }

    #[wasm_bindgen(js_name = getRules)]
    pub fn get_rules(&self) -> Result<String, JsError> {
        let rules = self.engine.get_rules();
        serde_json::to_string_pretty(rules)
            .map_err(|e| JsError::new(&format!("Serialization error: {}", e)))
    }
}

#[wasm_bindgen]
pub fn load_rules_from_json(json: &str) -> Result<JsValue, JsError> {
    let rules =
        workflow_serialize::json::from_json_str(json).map_err(|e| JsError::new(&e.to_string()))?;

    serde_wasm_bindgen::to_value(&rules)
        .map_err(|e| JsError::new(&format!("Serialization error: {}", e)))
}

#[wasm_bindgen]
pub fn load_rules_from_yaml(yaml: &str) -> Result<JsValue, JsError> {
    let rules =
        workflow_serialize::yaml::from_yaml_str(yaml).map_err(|e| JsError::new(&e.to_string()))?;

    serde_wasm_bindgen::to_value(&rules)
        .map_err(|e| JsError::new(&format!("Serialization error: {}", e)))
}

#[wasm_bindgen]
pub fn export_rules_to_json(rules_json: &str) -> Result<String, JsError> {
    let rules: Vec<workflow_domain::TriggerRule> = serde_json::from_str(rules_json)
        .map_err(|e| JsError::new(&format!("Invalid rules JSON: {}", e)))?;

    RuleExporter::to_json(&rules).map_err(|e| JsError::new(&e.to_string()))
}

#[wasm_bindgen]
pub fn export_rules_to_yaml(rules_json: &str) -> Result<String, JsError> {
    let rules: Vec<workflow_domain::TriggerRule> = serde_json::from_str(rules_json)
        .map_err(|e| JsError::new(&format!("Invalid rules JSON: {}", e)))?;

    RuleExporter::to_yaml(&rules).map_err(|e| JsError::new(&e.to_string()))
}

#[wasm_bindgen]
pub fn validate_rules(rules_json: &str) -> Result<JsValue, JsError> {
    let rules: Vec<workflow_domain::TriggerRule> = serde_json::from_str(rules_json)
        .map_err(|e| JsError::new(&format!("Invalid rules JSON: {}", e)))?;

    let result = workflow_domain::TriggerValidator::validate_all(&rules);

    serde_wasm_bindgen::to_value(&result)
        .map_err(|e| JsError::new(&format!("Serialization error: {}", e)))
}

#[derive(serde::Serialize)]
struct FlowResult {
    workflow: String,
    logs: Vec<String>,
    success: bool,
    error: Option<String>,
}

#[wasm_bindgen(js_name = executeFlow)]
pub fn execute_flow(source: &str, event_data_json: &str) -> Result<JsValue, JsError> {
    let program = FlowParser::parse_flow_program(source)
        .map_err(|e| JsError::new(&format!("Parse error: {}", e)))?;

    let event_data: serde_json::Value = serde_json::from_str(event_data_json)
        .map_err(|e| JsError::new(&format!("Invalid event data JSON: {}", e)))?;

    let mut evaluator = FlowEvaluator::new();
    evaluator.load_program(&program);

    let mut results = Vec::new();

    for workflow in &program.workflows {
        let context = TriggerContext::new(&workflow.event, event_data.clone());

        match evaluator.execute_workflow(workflow, &context) {
            Ok(logs) => {
                results.push(FlowResult {
                    workflow: workflow.name.clone(),
                    logs,
                    success: true,
                    error: None,
                });
            }
            Err(e) => {
                results.push(FlowResult {
                    workflow: workflow.name.clone(),
                    logs: Vec::new(),
                    success: false,
                    error: Some(e.to_string()),
                });
            }
        }
    }

    serde_wasm_bindgen::to_value(&results)
        .map_err(|e| JsError::new(&format!("Serialization error: {}", e)))
}

#[derive(serde::Serialize)]
struct FlowParseResult {
    functions: Vec<String>,
    workflows: Vec<WorkflowInfo>,
    imports: Vec<String>,
}

#[derive(serde::Serialize)]
struct WorkflowInfo {
    name: String,
    event: String,
    params: Vec<String>,
}

#[wasm_bindgen(js_name = parseFlow)]
pub fn parse_flow(source: &str) -> Result<JsValue, JsError> {
    let program = FlowParser::parse_flow_program(source)
        .map_err(|e| JsError::new(&format!("Parse error: {}", e)))?;

    let result = FlowParseResult {
        functions: program.functions.iter().map(|f| f.name.clone()).collect(),
        workflows: program
            .workflows
            .iter()
            .map(|w| WorkflowInfo {
                name: w.name.clone(),
                event: w.event.clone(),
                params: w.params.clone(),
            })
            .collect(),
        imports: program.imports.iter().map(|i| i.name.clone()).collect(),
    };

    serde_wasm_bindgen::to_value(&result)
        .map_err(|e| JsError::new(&format!("Serialization error: {}", e)))
}
