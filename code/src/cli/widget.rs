use async_trait::async_trait;
use crossterm::event::Event;

#[async_trait]
pub trait Widget {
    fn render(&self, start_row: u16);

    fn handle_key(&mut self, event: Event);

    async fn recv(&self) -> Option<String>;
}
