use std::io::{self, Write};

use async_trait::async_trait;
use crossterm::{
    cursor,
    event::{Event, KeyCode, KeyEventKind},
    execute,
    style::Print,
    terminal::{Clear, ClearType},
};
use futures::channel::oneshot;
use tokio::sync::mpsc;

use crate::cli::widget::Widget;

pub struct Approval {
    tool_name: String,
    message: String,
    result_tx: Option<oneshot::Sender<bool>>,
    done_tx: mpsc::UnboundedSender<String>,
}

impl Approval {
    pub fn new(
        tool_name: String,
        message: String,
        result_tx: oneshot::Sender<bool>,
        done_tx: mpsc::UnboundedSender<String>,
    ) -> Self {
        Approval {
            tool_name,
            message,
            result_tx: Some(result_tx),
            done_tx,
        }
    }

    fn finish(&mut self, approved: bool) {
        if let Some(result_tx) = self.result_tx.take() {
            let _ = result_tx.send(approved);
            let _ = self.done_tx.send(String::new());
        }
    }
}

#[async_trait]
impl Widget for Approval {
    fn render(&self, start_row: u16) {
        let mut stdout = io::stdout();

        execute!(
            stdout,
            cursor::MoveTo(0, start_row),
            Clear(ClearType::CurrentLine),
            Print(format!(
                "Approve tool {}: '{}'? [y/N]",
                self.tool_name, self.message
            )),
        )
        .unwrap();

        stdout.flush().unwrap();
    }

    fn handle_key(&mut self, event: Event) {
        let Event::Key(key) = event else {
            return;
        };

        if key.kind != KeyEventKind::Press {
            return;
        }

        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => self.finish(true),
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Enter => self.finish(false),
            _ => {}
        }
    }

    async fn recv(&self) -> Option<String> {
        std::future::pending().await
    }
}
