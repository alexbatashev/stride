use std::{
    io::{self, Write},
    sync::{Arc, Mutex},
    time::Instant,
};

use async_trait::async_trait;
use crossterm::{
    cursor,
    event::{Event, KeyCode, KeyEvent, KeyEventKind},
    execute,
    style::Print,
    terminal::{Clear, ClearType},
};
use tokio::{
    sync::mpsc,
    time::{Duration, sleep},
};

use crate::cli::widget::Widget;

pub struct Prompt {
    state: Arc<Mutex<PromptState>>,
    submitted_tx: mpsc::UnboundedSender<String>,
    submitted_rx: Arc<tokio::sync::Mutex<mpsc::UnboundedReceiver<String>>>,
}

#[derive(Default)]
struct PromptState {
    current_prompt: String,
    cursor: usize,
    spinner_started: Option<Instant>,
}

impl Prompt {
    pub fn new() -> Self {
        let (submitted_tx, submitted_rx) = mpsc::unbounded_channel();

        Prompt {
            state: Arc::new(Mutex::new(PromptState {
                current_prompt: String::with_capacity(2048),
                ..Default::default()
            })),
            submitted_tx,
            submitted_rx: Arc::new(tokio::sync::Mutex::new(submitted_rx)),
        }
    }

    pub fn charge_spinner(&self) {
        self.state.lock().unwrap().spinner_started = Some(Instant::now());
    }

    pub fn discharge_spinner(&self) {
        self.state.lock().unwrap().spinner_started = None;
    }

    pub fn height(&self) -> u16 {
        if self.state.lock().unwrap().spinner_started.is_some() {
            2
        } else {
            1
        }
    }

    pub async fn tick(&self) {
        if self.state.lock().unwrap().spinner_started.is_some() {
            sleep(Duration::from_secs(1)).await;
        } else {
            std::future::pending::<()>().await;
        }
    }

    pub fn handle_event(&self, event: Event) {
        let Event::Key(key) = event else {
            return;
        };

        if key.kind != KeyEventKind::Press {
            return;
        }

        self.handle_key_event(key);
    }

    fn handle_key_event(&self, key: KeyEvent) {
        let mut state = self.state.lock().unwrap();

        match key.code {
            KeyCode::Char(ch) => {
                let cursor = state.cursor;
                state.current_prompt.insert(cursor, ch);
                state.cursor += ch.len_utf8();
            }
            KeyCode::Backspace => {
                if let Some((idx, _)) = state.current_prompt[..state.cursor].char_indices().last() {
                    state.current_prompt.remove(idx);
                    state.cursor = idx;
                }
            }
            KeyCode::Left => {
                if let Some((idx, _)) = state.current_prompt[..state.cursor].char_indices().last() {
                    state.cursor = idx;
                }
            }
            KeyCode::Right if state.cursor < state.current_prompt.len() => {
                let ch = state.current_prompt[state.cursor..].chars().next().unwrap();
                state.cursor += ch.len_utf8();
            }
            KeyCode::Enter if state.spinner_started.is_none() => {
                let submitted = std::mem::take(&mut state.current_prompt);
                state.cursor = 0;
                self.submitted_tx.send(submitted).unwrap();
            }
            _ => {}
        }
    }
}

#[async_trait]
impl Widget for Prompt {
    fn render(&self, start_row: u16) {
        let state = self.state.lock().unwrap();
        let mut stdout = io::stdout();

        execute!(
            stdout,
            cursor::MoveTo(0, start_row),
            Clear(ClearType::CurrentLine),
            cursor::MoveTo(0, start_row + 1),
            Clear(ClearType::CurrentLine),
            cursor::MoveTo(0, start_row),
        )
        .unwrap();

        if let Some(started) = state.spinner_started {
            let seconds = started.elapsed().as_secs();
            execute!(
                stdout,
                Print(format!("Waiting ({seconds} seconds)")),
                cursor::MoveTo(0, start_row + 1),
                Clear(ClearType::CurrentLine),
            )
            .unwrap();
        }

        execute!(
            stdout,
            Print("> "),
            Print(&state.current_prompt),
            cursor::MoveTo(
                2 + state.cursor as u16,
                start_row + u16::from(state.spinner_started.is_some())
            ),
        )
        .unwrap();

        stdout.flush().unwrap();
    }

    fn handle_key(&mut self, event: Event) {
        self.handle_event(event);
    }

    async fn recv(&self) -> Option<String> {
        self.submitted_rx.lock().await.recv().await
    }
}
