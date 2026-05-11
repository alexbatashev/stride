use crossterm::event::KeyCode;

pub trait Widget {
    fn render(&self, start_row: u16);

    fn handle_key(&mut self, key: KeyCode);
}
