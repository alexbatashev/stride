use crossterm::{
    ExecutableCommand,
    style::{Color, ResetColor, SetForegroundColor},
};
use std::io::{self, Write, stdout};

fn print_colored(color: Color, f: impl FnOnce()) {
    let mut out = stdout();
    out.execute(SetForegroundColor(color)).unwrap();
    f();
    out.execute(ResetColor).unwrap();
}

pub fn print_thinking(thinking: &str) {
    print_colored(Color::DarkGrey, || {
        println!("\n💭 Thinking:\n{}\n", thinking)
    });
}

pub fn print_tool_call(name: &str) {
    print_colored(Color::Green, || match display_tool_name(name) {
        Some(name) => println!("🔧 Using tool: {}", name),
        None => println!("🔧 Using tool"),
    });
}

pub fn print_stream(content: &str) {
    print!("{}", content);
    io::stdout().flush().unwrap();
}

pub fn print_prompt(thread_id: &str) {
    print_colored(Color::Cyan, || print!("{}", prompt_text(thread_id)));
    io::stdout().flush().unwrap();
}

pub fn prompt_text(thread_id: &str) -> String {
    format!("\n[{}] ❯ ", shorten_thread_id(thread_id))
}

pub fn print_confirm_prompt(prompt: &str) {
    print_colored(Color::Magenta, || print!("{} [y/N] ", prompt));
    io::stdout().flush().unwrap();
}

pub fn print_error(msg: &str) {
    print_colored(Color::Red, || println!("❌ Error: {}", msg));
}

pub fn print_welcome(thread_id: &str) {
    print_colored(Color::Cyan, || {
        println!(
            "Friday Agent - thread {} - type your request or /help for commands",
            shorten_thread_id(thread_id)
        );
    });
}

pub fn print_help() {
    println!(
        r#"
Available commands:
  /quit, /q      - Exit the application
  /clear, /c     - Start a new conversation thread
  /model         - Change model
  /help, /h      - Show this help message

The assistant can:
  • Read files (read_file)
  • List directories (list_files)
  • Edit/create files (edit_file)
  • Execute shell commands (bash)

Type your coding request naturally and the assistant will help you!
"#
    );
}

pub fn print_thread_switched(thread_id: &str) {
    print_colored(Color::Yellow, || {
        println!("Started thread {}", shorten_thread_id(thread_id));
    });
}

pub fn print_threads(cwd: &str, rows: &[(String, i64, String)]) {
    println!("Threads for {}", cwd);
    if rows.is_empty() {
        println!("  (none)");
        return;
    }

    for (thread_id, updated_at, preview) in rows {
        println!(
            "{}  updated={}  {}",
            thread_id,
            updated_at,
            if preview.is_empty() {
                "(no preview)"
            } else {
                preview
            }
        );
    }
}

pub fn print_transcript(thread_id: &str, rows: &[(String, String, String, String, String)]) {
    for (role, content, thinking, tool_call_id, tool_name) in rows {
        match role.as_str() {
            "system" => {}
            "user" => {
                print_prompt(thread_id);
                println!("{}", content);
            }
            "assistant" => {
                if !thinking.is_empty() {
                    print_thinking(thinking);
                }
                if !content.is_empty() {
                    print!("\n🤖 ");
                    print_stream(content);
                    println!();
                }
            }
            "tool" => {
                if !tool_name.is_empty() {
                    print_tool_call(tool_name);
                } else if !tool_call_id.is_empty() {
                    print_tool_call(tool_call_id);
                }
                if !content.is_empty() {
                    print!("\n🤖 ");
                    print_stream(content);
                    println!();
                }
            }
            _ => {}
        }
    }
}

fn shorten_thread_id(thread_id: &str) -> &str {
    thread_id.get(..8).unwrap_or(thread_id)
}

fn display_tool_name(name: &str) -> Option<&str> {
    if name.is_empty()
        || name.starts_with("chatcmpl-tool-")
        || name.starts_with("toolu_")
        || name.starts_with("call_")
    {
        None
    } else {
        Some(name)
    }
}
