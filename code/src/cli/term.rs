use std::io::{self, Write};

use crossterm::{
    cursor,
    event::EventStream,
    execute,
    style::{Color, Print, ResetColor, SetForegroundColor},
    terminal::{self, Clear, ClearType},
};
use futures::{FutureExt, StreamExt, channel::oneshot};
use tokio::{select, sync::mpsc};

use super::{prompt::Prompt, widget::Widget};

pub struct Terminal {
    cmd_rx: mpsc::UnboundedReceiver<Command>,
    user_input_tx: mpsc::UnboundedSender<TermEvent>,
    prompt: Prompt,
    text_pos: (u16, u16),
}

#[derive(Debug, Clone)]
pub struct Choice {
    pub value: String,
    pub display: String,
    pub description: Option<String>,
}

pub enum TermEvent {
    Prompt(String),
    Escape,
}

pub struct TermInput {
    user_input_rx: mpsc::UnboundedReceiver<TermEvent>,
}

#[derive(Clone)]
pub struct TermOutput {
    cmd_tx: mpsc::UnboundedSender<Command>,
}

enum Command {
    Print {
        message: String,
        color: Option<Color>,
    },
    RequestApproval {
        message: String,
        result_tx: oneshot::Sender<bool>,
    },
}

impl Terminal {
    pub fn new() -> (TermInput, TermOutput, Self) {
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let (user_input_tx, user_input_rx) = mpsc::unbounded_channel();
        let input = TermInput { user_input_rx };
        let output = TermOutput { cmd_tx };
        let text_pos = cursor::position().expect("Failed to get cursor position");
        let terminal = Terminal {
            cmd_rx,
            user_input_tx,
            prompt: Prompt::new(),
            text_pos,
        };
        (input, output, terminal)
    }

    pub async fn run(mut self) {
        terminal::enable_raw_mode().unwrap();

        self.prompt.render(self.text_pos.1 + 1);

        let mut reader = EventStream::new();

        loop {
            let mut event = reader.next().fuse();

            select! {
                Some(cmd) = self.cmd_rx.recv() => {
                    match cmd {
                        Command::Print { message, color } => {
                            self.do_print(message, color)
                        },
                        Command::RequestApproval { message, result_tx } => {}
                    }
                }
                Some(Ok(event)) = event => {
                    self.prompt.handle_key(event);
                }
            }
        }
    }

    fn do_print(&mut self, message: String, color: Option<Color>) {
        let (cols, _) = terminal::size().expect("Failed to get terminal szie");
        let (col, row) = self.text_pos;
        let mut stdout = io::stdout();

        execute!(stdout, cursor::MoveTo(col, row)).unwrap();

        if !message.contains('\n') && cols - col > message.len() as u16 {
            if let Some(color) = color {
                execute!(
                    stdout,
                    SetForegroundColor(color),
                    Print(&message),
                    ResetColor
                )
                .unwrap();
            } else {
                execute!(stdout, Print(&message)).unwrap();
            }
            self.text_pos.0 += message.len() as u16;
        } else {
            execute!(stdout, Clear(ClearType::FromCursorDown)).unwrap();

            if let Some(color) = color {
                execute!(
                    stdout,
                    SetForegroundColor(color),
                    Print(&message),
                    ResetColor
                )
                .unwrap();
            } else {
                execute!(stdout, Print(&message)).unwrap();
            }

            for ch in message.chars() {
                if ch == '\n' {
                    self.text_pos.0 = 0;
                    self.text_pos.1 += 1;
                } else {
                    self.text_pos.0 += 1;

                    if self.text_pos.0 >= cols {
                        self.text_pos.0 = 0;
                        self.text_pos.1 += 1;
                    }
                }
            }

            self.prompt.render(self.text_pos.1 + 1);
        }

        stdout.flush().unwrap();
    }
}

impl TermInput {
    pub async fn recv(&mut self) -> Option<TermEvent> {
        self.user_input_rx.recv().await
    }
}

impl TermOutput {
    pub fn print(&self, message: &str, color: Option<Color>) {
        let command = Command::Print {
            message: message.to_string(),
            color,
        };

        // TODO handle errors
        self.cmd_tx.send(command).unwrap();
    }

    pub async fn request_approval(&self, message: &str, approved: oneshot::Sender<bool>) {
        let command = Command::RequestApproval {
            message: message.to_string(),
            result_tx: approved,
        };

        // TODO handle errors
        self.cmd_tx.send(command).unwrap()
    }
}
