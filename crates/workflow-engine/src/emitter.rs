use flume::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use workflow_domain::EngineEventPayload;

#[derive(Clone)]
pub struct EventEmitter {
    senders: Arc<Mutex<Vec<Sender<EngineEventPayload>>>>,
}

impl EventEmitter {
    pub fn new() -> Self {
        Self {
            senders: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn emit(&self, payload: EngineEventPayload) {
        let senders = self.senders.lock().unwrap();
        for sender in senders.iter() {
            let _ = sender.send(payload.clone());
        }
    }

    pub fn subscribe(&self) -> Receiver<EngineEventPayload> {
        let (sender, receiver) = flume::unbounded();
        let mut senders = self.senders.lock().unwrap();
        senders.push(sender);
        receiver
    }
}

impl Default for EventEmitter {
    fn default() -> Self {
        Self::new()
    }
}
