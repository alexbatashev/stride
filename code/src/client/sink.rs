use capnp::capability::Promise;
use capnp_rpc::pry;
use crossterm::style::Color;
use tokio::sync::mpsc;

use crate::{agent_capnp::event_sink, term::Stream};

pub struct EventSinkImpl {
    confirm_destructive: bool,
    confirm_tx: mpsc::UnboundedSender<String>,
    stream: Stream,
    had_text: bool,
}

impl EventSinkImpl {
    pub fn new(
        confirm_destructive: bool,
        confirm_tx: mpsc::UnboundedSender<String>,
        stream: Stream,
    ) -> Self {
        Self {
            confirm_destructive,
            confirm_tx,
            stream,
            had_text: false,
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
            if !self.had_text {
                self.stream.hide_spinner_now();
                self.stream
                    .write_colored_now("Assistant:\r\n", Some(Color::Cyan));
                self.had_text = true;
            }
            self.stream.write_now(text);
        }
        Promise::ok(())
    }

    fn on_thinking(
        &mut self,
        params: event_sink::OnThinkingParams,
        _: event_sink::OnThinkingResults,
    ) -> Promise<(), capnp::Error> {
        let text = pry!(pry!(params.get()).get_text()).to_str().unwrap_or("");
        self.stream
            .print_colored_now(&format!("Thinking:\n{text}"), Color::DarkGrey);
        Promise::ok(())
    }

    fn on_tool_call(
        &mut self,
        params: event_sink::OnToolCallParams,
        _: event_sink::OnToolCallResults,
    ) -> Promise<(), capnp::Error> {
        let name = pry!(pry!(params.get()).get_name()).to_str().unwrap_or("");
        if name.is_empty() || is_generated_tool_name(name) {
            self.stream.print_colored_now("Using tool", Color::Green);
        } else {
            self.stream
                .print_colored_now(&format!("Using tool: {name}"), Color::Green);
        }
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
        if self.had_text {
            self.stream.write_now("\r\n");
        }
        self.stream
            .print_colored_now(&format!("Error: {msg}"), Color::Red);
        self.had_text = false;
        Promise::ok(())
    }

    fn on_done(
        &mut self,
        _: event_sink::OnDoneParams,
        _: event_sink::OnDoneResults,
    ) -> Promise<(), capnp::Error> {
        if self.had_text {
            self.stream.write_now("\r\n");
        }
        self.had_text = false;
        Promise::ok(())
    }
}

fn is_generated_tool_name(name: &str) -> bool {
    name.starts_with("chatcmpl-tool-") || name.starts_with("toolu_") || name.starts_with("call_")
}
