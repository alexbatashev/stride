mod local;

pub use local::LocalAgent;

pub struct Message {}

pub trait CodeAgent {
    fn get_messages(&self) -> Vec<Message>;
}
