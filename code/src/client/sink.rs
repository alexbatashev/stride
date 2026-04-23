use capnp::capability::Promise;
use capnp_rpc::pry;
use tokio::sync::mpsc;

use crate::{agent_capnp::event_sink, cli};

pub struct EventSinkImpl {
    confirm_destructive: bool,
    confirm_tx: mpsc::UnboundedSender<String>,
    first_chunk: bool,
    had_output: bool,
}

impl EventSinkImpl {
    pub fn new(confirm_destructive: bool, confirm_tx: mpsc::UnboundedSender<String>) -> Self {
        Self {
            confirm_destructive,
            confirm_tx,
            first_chunk: true,
            had_output: false,
        }
    }
}

impl event_sink::Server for EventSinkImpl {
    fn on_text_chunk(
        &mut self,
        params: event_sink::OnTextChunkParams,
        _: event_sink::OnTextChunkResults,
    ) -> Promise<(), capnp::Error> {
        let text = pry!(pry!(params.get()).get_text()).to_str().unwrap_or("");
        if !text.is_empty() {
            if self.first_chunk {
                print!("\n🤖 ");
                self.first_chunk = false;
                self.had_output = true;
            }
            cli::print_stream(text);
        }
        Promise::ok(())
    }

    fn on_thinking(
        &mut self,
        params: event_sink::OnThinkingParams,
        _: event_sink::OnThinkingResults,
    ) -> Promise<(), capnp::Error> {
        let text = pry!(pry!(params.get()).get_text()).to_str().unwrap_or("");
        cli::print_thinking(text);
        Promise::ok(())
    }

    fn on_tool_call(
        &mut self,
        params: event_sink::OnToolCallParams,
        _: event_sink::OnToolCallResults,
    ) -> Promise<(), capnp::Error> {
        let name = pry!(pry!(params.get()).get_name()).to_str().unwrap_or("");
        cli::print_tool_call(name);
        Promise::ok(())
    }

    fn on_confirmation_required(
        &mut self,
        params: event_sink::OnConfirmationRequiredParams,
        _: event_sink::OnConfirmationRequiredResults,
    ) -> Promise<(), capnp::Error> {
        if !self.confirm_destructive {
            // Send empty string to signal "auto-approve"
            let _ = self.confirm_tx.send(String::new());
            return Promise::ok(());
        }
        let prompt = pry!(pry!(params.get()).get_prompt())
            .to_str()
            .unwrap_or("")
            .to_string();
        let _ = self.confirm_tx.send(prompt);
        Promise::ok(())
    }

    fn on_error(
        &mut self,
        params: event_sink::OnErrorParams,
        _: event_sink::OnErrorResults,
    ) -> Promise<(), capnp::Error> {
        let msg = pry!(pry!(params.get()).get_message())
            .to_str()
            .unwrap_or("");
        if self.had_output {
            println!();
        }
        cli::print_error(msg);
        self.first_chunk = true;
        self.had_output = false;
        Promise::ok(())
    }

    fn on_done(
        &mut self,
        _: event_sink::OnDoneParams,
        _: event_sink::OnDoneResults,
    ) -> Promise<(), capnp::Error> {
        if self.had_output {
            println!();
        }
        self.first_chunk = true;
        self.had_output = false;
        Promise::ok(())
    }
}
