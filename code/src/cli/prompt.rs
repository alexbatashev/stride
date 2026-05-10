use std::sync::Arc;

use tokio::sync::Mutex;

use crate::cli::widget::Widget;

pub struct Prompt {}

impl Widget for Prompt {
    fn render(&self, start_col: u64, start_row: u64) {
        todo!()
    }

    fn handle_key(&mut self, key: crossterm::event::KeyCode) {
        todo!()
    }
}
