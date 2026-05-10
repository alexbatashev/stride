use crossterm::style::Color;
use futures::channel::oneshot;
use tokio::sync::mpsc;

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
        let terminal = Terminal {
            cmd_rx,
            user_input_tx,
            history: Vec::new(),
            // spinner: Spinner::new(),
        };
        (input, output, terminal)
    }

    pub async fn run(mut self) {}
}

impl TermInput {
    pub async fn recv(&mut self) -> Option<String> {
        self.user_input_rx.recv().await
    }
}

impl TermOutput {
    pub async fn print(&self, message: &str) {}

    pub async fn request_approval(&self, message: &str, approved: oneshot::Sender<bool>) {
        // let (tx, rx) = oneshot::channel();

        todo!()
    }
}
