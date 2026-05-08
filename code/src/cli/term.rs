use crossterm::style::Color;
use tokio::sync::{mpsc, oneshot};

pub struct Terminal {
    cmd_rx: mpsc::UnboundedReceiver<Command>,
    history: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Choice {
    pub value: String,
    pub display: String,
    pub description: Option<String>,
}

#[derive(Clone)]
pub struct Stream {
    cmd_tx: mpsc::UnboundedSender<Command>,
}

enum Command {
    Print {
        message: String,
        color: Option<Color>,
        done_tx: Option<oneshot::Sender<()>>,
    },
    SetSpinner {
        active: bool,
        done_tx: Option<oneshot::Sender<()>>,
    },
    Prompt {
        response_tx: oneshot::Sender<Option<String>>,
    },
    Select {
        choices: Vec<Choice>,
        response_tx: oneshot::Sender<Option<Choice>>,
    },
}

impl Terminal {
    pub fn new() -> (Stream, Self) {
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let stream = Stream { cmd_tx };
        let terminal = Terminal {
            cmd_rx,
            history: Vec::new(),
            // spinner: Spinner::new(),
        };
        (stream, terminal)
    }

    pub async fn run(mut self) {}
}
