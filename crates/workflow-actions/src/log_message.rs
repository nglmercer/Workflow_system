use async_trait::async_trait;
use workflow_domain::{ActionParams, TriggerContext, WorkflowResult};
use workflow_engine::ActionHandler;

pub struct LogMessageHandler;

#[async_trait]
impl ActionHandler for LogMessageHandler {
    fn action_type(&self) -> &str {
        "log_message"
    }

    async fn execute(
        &self,
        params: &Option<ActionParams>,
        _context: &TriggerContext,
    ) -> WorkflowResult<serde_json::Value> {
        let message = params
            .as_ref()
            .and_then(|p| p.get("message"))
            .and_then(|v| v.as_str())
            .unwrap_or("No message");

        let level = params
            .as_ref()
            .and_then(|p| p.get("level"))
            .and_then(|v| v.as_str())
            .unwrap_or("info");

        match level {
            "error" => eprintln!("[ERROR] {}", message),
            "warn" => eprintln!("[WARN] {}", message),
            "debug" => println!("[DEBUG] {}", message),
            _ => println!("[INFO] {}", message),
        }

        Ok(serde_json::json!({
            "logged": true,
            "message": message,
            "level": level
        }))
    }
}
