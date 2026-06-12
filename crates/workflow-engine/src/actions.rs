use async_trait::async_trait;
use workflow_domain::{ActionParams, TriggerContext, WorkflowResult};

#[async_trait]
pub trait ActionHandler: Send + Sync {
    fn action_type(&self) -> &str;

    async fn execute(
        &self,
        params: &Option<ActionParams>,
        context: &TriggerContext,
    ) -> WorkflowResult<serde_json::Value>;
}
