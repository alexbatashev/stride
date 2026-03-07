use std::io::{self, BufRead, Write};

use friday::js::JavaScriptRuntime;

fn print_help() {
    println!("Commands:");
    println!("  .help        Show this help");
    println!("  .exit/.quit  Exit REPL");
    println!("Any other input is evaluated as JavaScript.");
}

fn write_line(text: &str) {
    let mut out = io::stdout();
    let _ = out.write_all(text.as_bytes());
    let _ = out.write_all(b"\n");
    let _ = out.flush();
}

fn evaluate_loop() -> Result<(), String> {
    let runtime = JavaScriptRuntime::new().map_err(|e| e.to_string())?;
    let context = runtime.make_context().map_err(|e| e.to_string())?;

    write_line("jskit-test REPL");
    write_line("Type .help for commands.");

    let stdin = io::stdin();
    let mut stdin = stdin.lock();

    loop {
        let mut out = io::stdout();
        let _ = out.write_all(b"> ");
        let _ = out.flush();

        let mut line = String::new();
        let read = stdin.read_line(&mut line).map_err(|e| e.to_string())?;
        if read == 0 {
            write_line("bye");
            return Ok(());
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if let Some(command) = trimmed.strip_prefix('.') {
            match command {
                "help" => {
                    print_help();
                    continue;
                }
                "exit" | "quit" => {
                    write_line("bye");
                    return Ok(());
                }
                _ => {}
            }
        }

        match context
            .evaluate(trimmed, "<repl>", 0, None)
            .and_then(|value| value.string())
        {
            Ok(output) => write_line(&output),
            Err(error) => write_line(&format!("error: {}", error)),
        }
    }
}

fn main() {
    if let Err(error) = evaluate_loop() {
        let _ = writeln!(io::stderr(), "failed to start repl: {}", error);
        std::process::exit(1);
    }
}
