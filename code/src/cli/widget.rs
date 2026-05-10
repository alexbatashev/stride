use crossterm::event::KeyCode;

pub trait Widget {
    fn render(&self, start_col: u64, start_row: u64);

    fn handle_key(&mut self, key: KeyCode);
}
