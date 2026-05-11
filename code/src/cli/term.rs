use std::io::{self, Write};

use crossterm::{
    cursor,
    event::{Event, EventStream, KeyCode},
    execute,
    style::{Color, Print, ResetColor, SetForegroundColor},
    terminal::{self, Clear, ClearType},
};
use futures::{FutureExt, StreamExt, channel::oneshot};
use tokio::{select, sync::mpsc};

use super::{approval::Approval, prompt::Prompt, widget::Widget};

pub struct Terminal {
    cmd_rx: mpsc::UnboundedReceiver<Command>,
    user_input_tx: mpsc::UnboundedSender<String>,
    prompt: Prompt,
    overlay_widget: Option<Box<dyn Widget>>,
    overlay_done_rx: Option<mpsc::UnboundedReceiver<String>>,
    text_pos: (u16, u16),
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
        tool_name: String,
        message: String,
        result_tx: oneshot::Sender<bool>,
    },
    ChargeSpinner,
    DischargeSpinner,
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
            overlay_widget: None,
            overlay_done_rx: None,
            text_pos,
        };
        (input, output, terminal)
    }

    pub async fn run(mut self) {
        terminal::enable_raw_mode().unwrap();

        self.render_prompt();

        let mut reader = EventStream::new();

        loop {
            let event = reader.next().fuse();
            let prompt = self.prompt.recv().fuse();
            let tick = self.prompt.tick().fuse();

            select! {
                Some(cmd) = self.cmd_rx.recv() => {
                    match cmd {
                        Command::Print { message, color } => {
                            self.do_print(message, color)
                        },
                        Command::RequestApproval { tool_name, message, result_tx } => {
                            let (done_tx, done_rx) = mpsc::unbounded_channel();
                            self.overlay_widget = Some(Box::new(Approval::new(
                                tool_name,
                                message,
                                result_tx,
                                done_tx,
                            )));
                            self.overlay_done_rx = Some(done_rx);
                            self.render_prompt();
                        }
                        Command::ChargeSpinner => {
                            self.prompt.charge_spinner();
                            self.render_prompt();
                        }
                        Command::DischargeSpinner => {
                            self.prompt.discharge_spinner();
                            self.render_prompt();
                        }
                    }
                }
                Some(input) = prompt => {
                    self.text_pos.0 = 0;
                    self.text_pos.1 += 1;
                    self.do_print(format!("> {input}\n"), None);
                    self.user_input_tx.send(input).unwrap();
                }
                _ = tick => {
                    if self.overlay_widget.is_none() {
                        self.render_prompt();
                    }
                }
                Some(Ok(event)) = event => {
                    if matches!(event, Event::Key(key) if key.code == KeyCode::Esc) {
                        break;
                    }

                    if let Some(widget) = self.overlay_widget.as_mut() {
                        widget.handle_key(event);
                        if let Some(rx) = self.overlay_done_rx.as_mut() {
                            if rx.try_recv().is_ok() {
                                self.overlay_widget = None;
                                self.overlay_done_rx = None;
                            }
                        }
                    } else {
                        self.prompt.handle_event(event);
                    }
                    self.render_prompt();
                }
            }
        }
    }

    fn render_prompt(&mut self) {
        let (_, rows) = terminal::size().expect("Failed to get terminal szie");
        let start_row = self.text_pos.1 + 1;
        let height = if self.overlay_widget.is_some() {
            1
        } else {
            self.prompt.height()
        };
        let end_row = start_row + height - 1;

        if end_row >= rows {
            let scroll_lines = end_row - rows + 1;
            let mut stdout = io::stdout();

            for _ in 0..scroll_lines {
                execute!(
                    stdout,
                    cursor::MoveTo(0, rows.saturating_sub(1)),
                    Print("\n")
                )
                .unwrap();
            }

            self.text_pos.1 = self.text_pos.1.saturating_sub(scroll_lines);
            stdout.flush().unwrap();
        }

        if let Some(widget) = self.overlay_widget.as_ref() {
            widget.render(self.text_pos.1 + 1);
        } else {
            self.prompt.render(self.text_pos.1 + 1);
        }
    }

    fn do_print(&mut self, message: String, color: Option<Color>) {
        let (cols, _) = terminal::size().expect("Failed to get terminal szie");
        let (col, row) = self.text_pos;
        let mut stdout = io::stdout();
        let clear_prompt = message.contains('\n') || cols - col <= message.len() as u16;

        execute!(stdout, cursor::MoveTo(col, row)).unwrap();

        if clear_prompt {
            execute!(stdout, Clear(ClearType::FromCursorDown)).unwrap();
        }

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

        self.render_prompt();
        stdout.flush().unwrap();
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

    pub async fn request_approval(
        &self,
        tool_name: &str,
        message: &str,
        approved: oneshot::Sender<bool>,
    ) {
        let command = Command::RequestApproval {
            tool_name: tool_name.to_string(),
            message: message.to_string(),
            result_tx: approved,
        };

        // TODO handle errors
        self.cmd_tx.send(command).unwrap()
    }

    pub fn charge_spinner(&self) {
        self.cmd_tx.send(Command::ChargeSpinner).unwrap();
    }

    pub fn discharge_spinner(&self) {
        self.cmd_tx.send(Command::DischargeSpinner).unwrap();
    }
}
