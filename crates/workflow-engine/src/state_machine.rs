use serde::{Deserialize, Serialize};

/// Workflow execution state
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum WorkflowState {
    /// Workflow is created but not started
    #[default]
    Pending,
    /// Workflow is actively running
    Running,
    /// Waiting for a specific event/signal
    WaitingForEvent(String),
    /// Waiting for a timer/delay
    WaitingForTimer(u64),
    /// Paused by user or system
    Paused,
    /// Workflow completed successfully
    Completed,
    /// Workflow failed with error
    Failed(String),
    /// Workflow was cancelled
    Cancelled,
    /// Workflow is being retried after failure
    Retrying { attempt: u32, max_attempts: u32 },
}

impl WorkflowState {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            WorkflowState::Completed | WorkflowState::Failed(_) | WorkflowState::Cancelled
        )
    }

    pub fn is_active(&self) -> bool {
        !self.is_terminal()
    }
}

/// State transition event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateTransition {
    pub from: WorkflowState,
    pub to: WorkflowState,
    pub timestamp: i64,
    pub metadata: Option<serde_json::Value>,
}

/// State machine tracker for workflow execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateMachine {
    pub current: WorkflowState,
    pub history: Vec<StateTransition>,
    pub context: serde_json::Value,
}

impl StateMachine {
    pub fn new(initial_state: WorkflowState, context: serde_json::Value) -> Self {
        Self {
            current: initial_state,
            history: Vec::new(),
            context,
        }
    }

    pub fn transition(
        &mut self,
        to: WorkflowState,
        metadata: Option<serde_json::Value>,
    ) -> WorkflowResult<()> {
        if self.current.is_terminal() {
            return Err(WorkflowError::InvalidTransition(format!(
                "Cannot transition from terminal state {:?}",
                self.current
            )));
        }

        let transition = StateTransition {
            from: self.current.clone(),
            to: to.clone(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            metadata,
        };

        self.history.push(transition);
        self.current = to;
        Ok(())
    }

    pub fn start(&mut self) -> WorkflowResult<()> {
        self.transition(WorkflowState::Running, None)
    }

    pub fn complete(&mut self) -> WorkflowResult<()> {
        self.transition(WorkflowState::Completed, None)
    }

    pub fn fail(&mut self, error: String) -> WorkflowResult<()> {
        self.transition(WorkflowState::Failed(error), None)
    }

    pub fn cancel(&mut self) -> WorkflowResult<()> {
        self.transition(WorkflowState::Cancelled, None)
    }

    pub fn wait_for_event(&mut self, event: String) -> WorkflowResult<()> {
        self.transition(WorkflowState::WaitingForEvent(event), None)
    }

    pub fn retry(&mut self, attempt: u32, max_attempts: u32) -> WorkflowResult<()> {
        self.transition(
            WorkflowState::Retrying {
                attempt,
                max_attempts,
            },
            None,
        )
    }

    pub fn pause(&mut self) -> WorkflowResult<()> {
        self.transition(WorkflowState::Paused, None)
    }

    pub fn resume(&mut self) -> WorkflowResult<()> {
        self.transition(WorkflowState::Running, None)
    }
}

use workflow_domain::{WorkflowError, WorkflowResult};

impl Default for StateMachine {
    fn default() -> Self {
        Self::new(WorkflowState::Pending, serde_json::Value::Null)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_transitions() {
        let mut sm = StateMachine::default();
        assert_eq!(sm.current, WorkflowState::Pending);

        sm.start().unwrap();
        assert_eq!(sm.current, WorkflowState::Running);

        sm.complete().unwrap();
        assert_eq!(sm.current, WorkflowState::Completed);
        assert!(sm.current.is_terminal());
    }

    #[test]
    fn test_cannot_transition_from_terminal() {
        let mut sm = StateMachine::default();
        sm.start().unwrap();
        sm.complete().unwrap();

        let result = sm.start();
        assert!(result.is_err());
    }

    #[test]
    fn test_retry_flow() {
        let mut sm = StateMachine::default();
        sm.start().unwrap();

        sm.retry(1, 3).unwrap();
        assert_eq!(
            sm.current,
            WorkflowState::Retrying {
                attempt: 1,
                max_attempts: 3
            }
        );

        sm.start().unwrap();
        assert_eq!(sm.current, WorkflowState::Running);
    }
}
