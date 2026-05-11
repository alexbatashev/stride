use std::io;

use crossterm::{style::Color, terminal};
use futures::channel::oneshot;
use termimad::crossterm::cursor;
use tokio::{select, sync::mpsc};

use super::prompt::Prompt;

pub struct Terminal {
    cmd_rx: mpsc::UnboundedReceiver<Command>,
    user_input_tx: mpsc::UnboundedSender<String>,
    prompt: Prompt,
    text_pos: (u16, u16),
}

#[derive(Debug, Clone)]
pub struct Choice {
    pub value: String,
    pub display: String,
    pub description: Option<String>,
}

pub struct TermInput {
    user_input_rx: mpsc::UnboundedReceiver<String>,
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

        loop {
            select! {
                Some(cmd) = self.cmd_rx.recv() => {
                    match cmd {
                        Command::Print { message, color } => {
                            self.do_print(message, color)
                        },
                        Command::RequestApproval { message, result_tx } => {}
                    }
                }
            }
        }
    }

    fn do_print(&mut self, message: String, color: Option<Color>) {
        let (cols, _) = terminal::size().expect("Failed to get terminal szie");
        let (col, row) = self.text_pos;

        // if ()
    }
}

impl TermInput {
    pub async fn recv(&mut self) -> Option<String> {
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
