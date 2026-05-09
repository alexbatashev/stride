use crossterm::style::Color;
use tokio::sync::{mpsc, oneshot};

pub struct Terminal {
    cmd_rx: mpsc::UnboundedReceiver<Command>,
    user_input_tx: mpsc::UnboundedSender<String>,
    history: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Choice {
    pub value: String,
    pub display: String,
    pub description: Option<String>,
}

pub struct Stream {
    cmd_tx: mpsc::UnboundedSender<Command>,
    user_input_rx: mpsc::UnboundedReceiver<String>,
}

enum Command {
    Print {
        message: String,
        color: Option<Color>,
        done_tx: Option<oneshot::Sender<()>>,
    },
}

impl Terminal {
    pub fn new() -> (Stream, Self) {
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let (user_input_tx, user_input_rx) = mpsc::unbounded_channel();
        let stream = Stream {
            cmd_tx,
            user_input_rx,
        };
        let terminal = Terminal {
            cmd_rx,
            user_input_tx,
            history: Vec::new(),
            // spinner: Spinner::new(),
        };
        (stream, terminal)
    }

    pub async fn run(mut self) {}
}

impl Stream {
    pub async fn recv(&mut self) -> Option<String> {
        self.user_input_rx.recv().await
    }

    pub async fn print(&mut self) {}
}
