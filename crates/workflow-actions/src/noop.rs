use async_trait::async_trait;
use workflow_domain::{ActionParams, TriggerContext, WorkflowResult};
use workflow_engine::ActionHandler;

pub struct NoopHandler;

#[async_trait]
impl ActionHandler for NoopHandler {
    fn action_type(&self) -> &str {
        "noop"
    }

    async fn execute(
        &self,
        _params: &Option<ActionParams>,
        _context: &TriggerContext,
    ) -> WorkflowResult<serde_json::Value> {
        Ok(serde_json::json!({ "noop": true }))
    }
}
