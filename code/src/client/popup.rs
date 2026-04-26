use std::future::Future;
use std::io::{self, Write};

use anyhow::Result;
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute, queue,
    style::{Color, Print, ResetColor, SetForegroundColor},
    terminal::{self, Clear, ClearType},
};

#[derive(Debug, Clone)]
pub struct PopupItem {
    pub value: String,
    pub display: String,
}

pub struct ReadLineResult {
    pub text: Option<String>,
    pub loaded_items: Option<Vec<PopupItem>>,
}

pub async fn read_line<F, Fut>(
    prompt: &str,
    initial_items: Option<&[PopupItem]>,
    load_items: F,
) -> Result<ReadLineResult>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<Vec<PopupItem>>>,
{
    let mut stdout = io::stdout();
    let raw_mode = RawMode::new()?;
    let (prompt_prefix, prompt_line) = split_prompt(prompt);
    if !prompt_prefix.is_empty() {
        queue!(stdout, Print(prompt_prefix))?;
        stdout.flush()?;
    }

    let mut buffer = String::new();
    let mut selected = 0usize;
    let mut popup_lines = 0usize;
    let mut loaded_items = None;
    let mut items = initial_items.map(|items| items.to_vec());
    let mut load_items = Some(load_items);

    render(
        &mut stdout,
        prompt_line,
        &buffer,
        &[],
        selected,
        popup_lines,
    )?;
    loop {
        if buffer == "/model" && items.is_none() {
            raw_mode.suspend()?;
            clear_popup(&mut stdout, popup_lines)?;
            println!("\nLoading models...");
            stdout.flush()?;
            let loader = load_items.take().expect("model loader is single use");
            let result = loader().await;
            raw_mode.resume()?;
            let loaded = result?;
            items = Some(loaded.clone());
            loaded_items = Some(loaded);
            popup_lines = 0;
        }

        let matches = popup_matches(&buffer, items.as_deref().unwrap_or(&[]));
        if selected >= matches.len() {
            selected = 0;
        }
        render(
            &mut stdout,
            prompt_line,
            &buffer,
            &matches,
            selected,
            popup_lines,
        )?;
        popup_lines = matches.len().min(8);

        match event::read()? {
            Event::Key(KeyEvent {
                code: KeyCode::Char('c'),
                modifiers: KeyModifiers::CONTROL,
                ..
            })
            | Event::Key(KeyEvent {
                code: KeyCode::Char('d'),
                modifiers: KeyModifiers::CONTROL,
                ..
            }) => {
                finish(&mut stdout, popup_lines)?;
                return Ok(ReadLineResult {
                    text: None,
                    loaded_items,
                });
            }
            Event::Key(KeyEvent {
                code: KeyCode::Char(ch),
                ..
            }) => buffer.push(ch),
            Event::Key(KeyEvent {
                code: KeyCode::Backspace,
                ..
            }) => {
                buffer.pop();
            }
            Event::Key(KeyEvent {
                code: KeyCode::Enter,
                ..
            }) => {
                if is_model_input(&buffer) {
                    if let Some(item) = matches.get(selected) {
                        buffer = format!("/model {}", item.value);
                    }
                }
                finish(&mut stdout, popup_lines)?;
                return Ok(ReadLineResult {
                    text: Some(buffer),
                    loaded_items,
                });
            }
            Event::Key(KeyEvent {
                code: KeyCode::Down,
                ..
            })
            | Event::Key(KeyEvent {
                code: KeyCode::Tab, ..
            }) => {
                if !matches.is_empty() {
                    selected = (selected + 1) % matches.len();
                }
            }
            Event::Key(KeyEvent {
                code: KeyCode::Up, ..
            }) => {
                if !matches.is_empty() {
                    selected = selected.checked_sub(1).unwrap_or(matches.len() - 1);
                }
            }
            _ => {}
        }
    }
}

fn split_prompt(prompt: &str) -> (&str, &str) {
    match prompt.rfind('\n') {
        Some(idx) => prompt.split_at(idx + 1),
        None => ("", prompt),
    }
}

struct RawMode;

impl RawMode {
    fn new() -> Result<Self> {
        terminal::enable_raw_mode()?;
        Ok(Self)
    }

    fn suspend(&self) -> Result<()> {
        terminal::disable_raw_mode()?;
        Ok(())
    }

    fn resume(&self) -> Result<()> {
        terminal::enable_raw_mode()?;
        Ok(())
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
    }
}

fn popup_matches<'a>(buffer: &str, items: &'a [PopupItem]) -> Vec<&'a PopupItem> {
    if !is_model_input(buffer) {
        return Vec::new();
    }

    let query = buffer.strip_prefix("/model").unwrap_or("").trim();
    let mut matches: Vec<_> = items
        .iter()
        .filter_map(|item| fuzzy_score(&item.value, query).map(|score| (score, item)))
        .collect();
    matches.sort_by(|(left_score, left), (right_score, right)| {
        left_score
            .cmp(right_score)
            .then_with(|| left.value.cmp(&right.value))
    });
    matches.into_iter().map(|(_, item)| item).collect()
}

fn is_model_input(buffer: &str) -> bool {
    buffer == "/model" || buffer.starts_with("/model ")
}

fn fuzzy_score(candidate: &str, query: &str) -> Option<usize> {
    if query.is_empty() {
        return Some(0);
    }

    let mut score = 0usize;
    let mut last = 0usize;
    let mut chars = candidate
        .char_indices()
        .map(|(idx, ch)| (idx, ch.to_ascii_lowercase()));
    for needle in query.chars().map(|ch| ch.to_ascii_lowercase()) {
        let (idx, _) = chars.find(|(_, ch)| *ch == needle)?;
        score += idx.saturating_sub(last);
        last = idx;
    }
    Some(score)
}

fn render(
    stdout: &mut io::Stdout,
    prompt: &str,
    buffer: &str,
    matches: &[&PopupItem],
    selected: usize,
    previous_popup_lines: usize,
) -> Result<()> {
    queue!(
        stdout,
        cursor::MoveToColumn(0),
        Clear(ClearType::CurrentLine)
    )?;
    queue!(stdout, Print(prompt), Print(buffer))?;

    let visible = matches.len().min(8);
    for (idx, item) in matches.iter().take(visible).enumerate() {
        queue!(stdout, Print("\r\n"), Clear(ClearType::CurrentLine))?;
        if idx == selected {
            queue!(
                stdout,
                SetForegroundColor(Color::Cyan),
                Print("> "),
                ResetColor
            )?;
        } else {
            queue!(stdout, Print("  "))?;
        }
        queue!(stdout, Print(&item.display))?;
    }

    for _ in visible..previous_popup_lines {
        queue!(stdout, Print("\r\n"), Clear(ClearType::CurrentLine))?;
    }

    if previous_popup_lines > 0 || visible > 0 {
        queue!(
            stdout,
            cursor::MoveUp(previous_popup_lines.max(visible) as u16)
        )?;
    }
    queue!(
        stdout,
        cursor::MoveToColumn((prompt.chars().count() + buffer.chars().count()) as u16)
    )?;
    stdout.flush()?;
    Ok(())
}

fn clear_popup(stdout: &mut io::Stdout, popup_lines: usize) -> Result<()> {
    for _ in 0..popup_lines {
        queue!(
            stdout,
            cursor::MoveDown(1),
            cursor::MoveToColumn(0),
            Clear(ClearType::CurrentLine)
        )?;
    }
    if popup_lines > 0 {
        queue!(stdout, cursor::MoveUp(popup_lines as u16))?;
    }
    stdout.flush()?;
    Ok(())
}

fn finish(stdout: &mut io::Stdout, popup_lines: usize) -> Result<()> {
    clear_popup(stdout, popup_lines)?;
    execute!(stdout, Print("\r\n"))?;
    Ok(())
}
