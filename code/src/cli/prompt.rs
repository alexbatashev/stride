use std::io::{self, Write};

use crossterm::{
    cursor,
    event::{Event, KeyCode, KeyEvent, KeyEventKind},
    execute,
    style::Print,
    terminal::{Clear, ClearType},
};
use tokio::sync::mpsc;

use crate::cli::widget::Widget;

pub struct Prompt {
    current_prompt: String,
    cursor: usize,
    submitted_tx: mpsc::UnboundedSender<String>,
    submitted_rx: mpsc::UnboundedReceiver<String>,
}

impl Prompt {
    pub fn new() -> Self {
        let (submitted_tx, submitted_rx) = mpsc::unbounded_channel();

        Prompt {
            current_prompt: String::with_capacity(2048),
            cursor: 0,
            submitted_tx,
            submitted_rx,
        }
    }
    
    pub fn charge_spinner(&mut self) {}
    pub fn discharge_spinner(&mut self) {}

    fn handle_event(&mut self, event: Event) {
        let Event::Key(key) = event else {
            return;
        };

        if key.kind != KeyEventKind::Press {
            return;
        }

        self.handle_key_event(key);
    }

    fn handle_key_event(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char(ch) => {
                self.current_prompt.insert(self.cursor, ch);
                self.cursor += ch.len_utf8();
            }
            KeyCode::Backspace => {
                if let Some((idx, _)) = self.current_prompt[..self.cursor].char_indices().last() {
                    self.current_prompt.remove(idx);
                    self.cursor = idx;
                }
            }
            KeyCode::Left => {
                if let Some((idx, _)) = self.current_prompt[..self.cursor].char_indices().last() {
                    self.cursor = idx;
                }
            }
            KeyCode::Right => {
                if self.cursor < self.current_prompt.len() {
                    let ch = self.current_prompt[self.cursor..].chars().next().unwrap();
                    self.cursor += ch.len_utf8();
                }
            }
            KeyCode::Enter => {
                let submitted = std::mem::take(&mut self.current_prompt);
                self.cursor = 0;
                self.submitted_tx.send(submitted).unwrap();
            }
            _ => {}
        }
    }
}

impl Widget for Prompt {
    fn render(&self, start_row: u16) {
        let mut stdout = io::stdout();

        execute!(
            stdout,
            cursor::MoveTo(0, start_row),
            Clear(ClearType::CurrentLine),
            Print("> "),
            Print(&self.current_prompt),
            cursor::MoveTo(2 + self.cursor as u16, start_row),
        )
        .unwrap();

        stdout.flush().unwrap();
    }

    fn handle_key(&mut self, event: Event) {
        self.handle_event(event);
    }

    async fn recv(&mut self) -> Option<String> {
        self.submitted_rx.recv().await
    }
}
