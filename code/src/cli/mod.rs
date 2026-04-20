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
    print_colored(Color::DarkGrey, || println!("\n💭 Thinking:\n{}\n", thinking));
}

pub fn print_tool_call(name: &str) {
    print_colored(Color::Green, || println!("🔧 Using tool: {}", name));
}

pub fn print_stream(content: &str) {
    print!("{}", content);
    io::stdout().flush().unwrap();
}

pub fn prompt_user() -> Option<String> {
    print_colored(Color::Cyan, || print!("\n❯ "));
    io::stdout().flush().unwrap();

    let mut input = String::new();
    match io::stdin().read_line(&mut input).unwrap() {
        0 => None,
        _ => Some(input.trim().to_string()),
    }
}

pub fn confirm(prompt: &str) -> bool {
    print_colored(Color::Magenta, || print!("{} [y/N] ", prompt));
    io::stdout().flush().unwrap();

    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();
    let input = input.trim().to_lowercase();
    input == "y" || input == "yes"
}

pub fn print_error(msg: &str) {
    print_colored(Color::Red, || println!("❌ Error: {}", msg));
}

pub fn print_warning(msg: &str) {
    print_colored(Color::Yellow, || println!("⚠️  Warning: {}", msg));
}

pub fn print_info(msg: &str) {
    print_colored(Color::Blue, || println!("ℹ️  {}", msg));
}

pub fn print_welcome() {
    print_colored(Color::Cyan, || {
        println!("Friday Agent - Type your request or /help for commands");
    });
}

pub fn print_help() {
    println!(
        r#"
Available commands:
  /quit, /q      - Exit the application
  /clear, /c     - Clear the conversation history
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
