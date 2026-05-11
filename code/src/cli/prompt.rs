use std::sync::Arc;

use tokio::sync::Mutex;

use crate::cli::widget::Widget;

pub struct Prompt {}

impl Prompt {
    pub fn new() -> Self {
        Prompt {}
    }
}

impl Widget for Prompt {
    fn render(&self, start_row: u16) {
        todo!()
    }

    fn handle_key(&mut self, key: crossterm::event::KeyCode) {
        todo!()
    }
}
