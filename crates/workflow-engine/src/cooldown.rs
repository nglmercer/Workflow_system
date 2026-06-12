use std::collections::HashMap;
use std::time::Instant;

pub struct CooldownTracker {
    last_executions: HashMap<String, Instant>,
}

impl CooldownTracker {
    pub fn new() -> Self {
        Self {
            last_executions: HashMap::new(),
        }
    }

    pub fn is_in_cooldown(&self, rule_id: &str, cooldown_ms: u64) -> bool {
        if let Some(last) = self.last_executions.get(rule_id) {
            let elapsed = last.elapsed().as_millis() as u64;
            elapsed < cooldown_ms
        } else {
            false
        }
    }

    pub fn record_execution(&mut self, rule_id: &str) {
        self.last_executions
            .insert(rule_id.to_string(), Instant::now());
    }
}

impl Default for CooldownTracker {
    fn default() -> Self {
        Self::new()
    }
}
