use std::io::{self, BufRead, Write};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use friday::chat::{
    ChatMessage, ChatProviderConfiguration, ChatProviderKind, ChatService, ChatStorage,
    DirectChatTransport, NullChatStorage, TurnRole,
};
use friday::tools::{JSTool, Tool};
use futures::StreamExt;
use uuid::Uuid;

#[tokio::main]
async fn main() {
    let mut provider = ChatProviderConfiguration {
        id: Uuid::new_v4(),
        name: "Local Ollama".to_owned(),
        kind: ChatProviderKind::Ollama,
        base_url: "http://localhost:11434".to_owned(),
        token: String::new(),
        default_model: String::new(),
    };
    let provider_id = provider.id.to_string();

    let transport = Arc::new(DirectChatTransport::from_provider(provider.clone()));
    let storage: Arc<dyn ChatStorage> = Arc::new(NullChatStorage);
    let chat = ChatService::new(vec![transport], storage);
    let js_tool: Arc<dyn Tool> = Arc::new(JSTool::new());
    let mut tools_enabled = false;

    let models = chat.list_models().await;
    let Some(model) = models.first().map(|m| m.model.clone()) else {
        write_stderr(&format!(
            "No Ollama models found at {}. Pull one first (for example: `ollama pull llama3.2`).\n",
            provider.base_url
        ));
        return;
    };

    provider.default_model = model.clone();
    chat.set_model(provider_id.clone(), model.clone()).await;

    println!("Friday CLI");
    println!("Provider: {} ({})", provider.name, provider.base_url);
    println!("Model: {}", model);
    println!("Type your prompt and press Enter. Type `exit` to quit.");
    println!("Type `/tools on` or `/tools off` to toggle tool execution (default: off).");

    let thread_id = Uuid::new_v4();
    let stdin = io::stdin();
    let mut stdin = stdin.lock();

    loop {
        write_stdout("> ");
        let mut line = String::new();
        let Ok(read) = stdin.read_line(&mut line) else {
            println!();
            break;
        };
        if read == 0 {
            println!();
            break;
        }

        let prompt = line.trim().to_owned();
        if prompt.is_empty() {
            continue;
        }
        if prompt.eq_ignore_ascii_case("exit") {
            break;
        }
        if prompt.eq_ignore_ascii_case("/tools on") {
            tools_enabled = true;
            println!("Tools enabled.");
            continue;
        }
        if prompt.eq_ignore_ascii_case("/tools off") {
            tools_enabled = false;
            println!("Tools disabled.");
            continue;
        }

        let now = now_millis();
        let user_turn = ChatMessage {
            id: Uuid::new_v4(),
            thread_id,
            user_id: None,
            parent_id: None,
            provider_id: provider_id.clone(),
            model_id: model.clone(),
            model_name: model.clone(),
            role: TurnRole::User,
            thinking: None,
            content: prompt,
            tool_call: None,
            tool_result: None,
            created_at: now,
            updated_at: now,
            is_done: false,
            usage: None,
        };

        let mut printed_count = 0usize;
        let mut printed_thinking_count = 0usize;
        let mut thinking_started = false;
        let mut thinking_line_open = false;
        let mut active_message_id: Option<Uuid> = None;

        write_stdout("friday> ");

        let tools = if tools_enabled {
            vec![js_tool.clone()]
        } else {
            Vec::new()
        };

        let mut stream = chat.add_message(tools, user_turn).await;
        while let Some(item) = stream.next().await {
            match item {
                Ok(partial) => {
                    if active_message_id != Some(partial.id) {
                        active_message_id = Some(partial.id);
                        printed_count = 0;
                        printed_thinking_count = 0;
                        if thinking_line_open {
                            write_stderr("\n");
                            thinking_line_open = false;
                        }
                        thinking_started = false;
                    }

                    let full_thinking = partial.thinking.unwrap_or_default();
                    let thinking_suffix = safe_suffix(&full_thinking, printed_thinking_count);
                    if !thinking_suffix.is_empty() {
                        if !thinking_started {
                            write_stderr("thinking> ");
                            thinking_started = true;
                            thinking_line_open = true;
                        }
                        write_stderr(thinking_suffix);
                        printed_thinking_count = full_thinking.chars().count();
                    }

                    let full_text = partial.content;
                    let suffix = safe_suffix(&full_text, printed_count);
                    if !suffix.is_empty() {
                        if thinking_line_open {
                            write_stderr("\n");
                            thinking_line_open = false;
                        }
                        write_stdout(suffix);
                        printed_count = full_text.chars().count();
                    }
                }
                Err(error) => {
                    if thinking_line_open {
                        write_stderr("\n");
                    }
                    println!();
                    write_stderr(&format!("Request failed: {}\n", error));
                    continue;
                }
            }
        }

        if thinking_line_open {
            write_stderr("\n");
        }
        println!();
    }
}

fn safe_suffix(s: &str, char_offset: usize) -> &str {
    match s.char_indices().nth(char_offset) {
        Some((idx, _)) => &s[idx..],
        None => "",
    }
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

fn write_stdout(text: &str) {
    let mut out = io::stdout();
    let _ = out.write_all(text.as_bytes());
    let _ = out.flush();
}

fn write_stderr(text: &str) {
    let mut out = io::stderr();
    let _ = out.write_all(text.as_bytes());
    let _ = out.flush();
}
