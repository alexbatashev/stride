use tokio::sync::mpsc;

pub struct Stream {
    cmd_tx: mpsc::Sender<Command>,
}

pub struct Terminal {
    cmd_rx: mpsc::Receiver<Command>, 
}

enum Command {
    Print(String),
    ShowPrompt,
    HidePrompt,
}

impl Stream {
    pub async fn print(&self, message: &str) {
        let _ = self.cmd_tx.send(Command::Print(message.to_owned())).await;
    }
}

impl Terminal {
    pub fn new() -> (Stream, Self) {
        let (cmd_tx, cmd_rx) = mpsc::channel(20);
        let stream = Stream { cmd_tx };
        let terminal = Terminal {
            cmd_rx
        };

        (stream, terminal)
    }

    pub async fn run(mut self) {
        loop {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => { break },
                Some(cmd) = self.cmd_rx.recv() => {
                    match cmd {
                        Command::Print(msg) => termimad::print_inline(&msg),
                        _ => {}
                    }
                }
            }
        }
    }
}
