pub mod log_message;
pub mod noop;
pub mod set_var;

use workflow_engine::ActionHandler;

pub fn builtin_handlers() -> Vec<Box<dyn ActionHandler>> {
    vec![
        Box::new(log_message::LogMessageHandler),
        Box::new(set_var::SetVarHandler),
        Box::new(noop::NoopHandler),
    ]
}
