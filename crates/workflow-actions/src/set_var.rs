use async_trait::async_trait;
use workflow_domain::{ActionParams, TriggerContext, WorkflowResult};
use workflow_engine::ActionHandler;

pub struct SetVarHandler;

#[async_trait]
impl ActionHandler for SetVarHandler {
    fn action_type(&self) -> &str {
        "set_var"
    }

    async fn execute(
        &self,
        params: &Option<ActionParams>,
        _context: &TriggerContext,
    ) -> WorkflowResult<serde_json::Value> {
        let key = params
            .as_ref()
            .and_then(|p| p.get("key"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let value = params
            .as_ref()
            .and_then(|p| p.get("value"))
            .cloned()
            .unwrap_or(serde_json::Value::Null);

        Ok(serde_json::json!({
            "var_set": true,
            "key": key,
            "value": value
        }))
    }
}
