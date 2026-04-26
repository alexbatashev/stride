use std::{
    io::{self, Write},
    time::Duration,
};

use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    queue,
    style::{Color, Print, ResetColor, SetForegroundColor},
    terminal::{self, Clear, ClearType},
};
use tokio::{
    sync::{mpsc, oneshot},
    time,
};

#[derive(Clone)]
pub struct Stream {
    cmd_tx: mpsc::UnboundedSender<Command>,
}

pub struct Terminal {
    cmd_rx: mpsc::UnboundedReceiver<Command>,
    history: Vec<String>,
    spinner: Spinner,
}

#[derive(Debug, Clone)]
pub struct Choice {
    pub value: String,
    pub display: String,
    pub description: Option<String>,
}

enum Command {
    Print {
        message: String,
        color: Option<Color>,
        done_tx: Option<oneshot::Sender<()>>,
    },
    Write {
        text: String,
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

impl Stream {
    pub async fn print(&self, message: &str) {
        let (done_tx, done_rx) = oneshot::channel();
        let _ = self.cmd_tx.send(Command::Print {
            message: message.to_owned(),
            color: None,
            done_tx: Some(done_tx),
        });
        let _ = done_rx.await;
    }

    pub fn print_colored_now(&self, message: &str, color: Color) {
        let _ = self.cmd_tx.send(Command::Print {
            message: message.to_owned(),
            color: Some(color),
            done_tx: None,
        });
    }

    pub fn write_now(&self, text: &str) {
        self.write_colored_now(text, None);
    }

    pub fn write_colored_now(&self, text: &str, color: Option<Color>) {
        let _ = self.cmd_tx.send(Command::Write {
            text: text.to_owned(),
            color,
            done_tx: None,
        });
    }

    pub async fn show_spinner(&self) {
        let (done_tx, done_rx) = oneshot::channel();
        let _ = self.cmd_tx.send(Command::SetSpinner {
            active: true,
            done_tx: Some(done_tx),
        });
        let _ = done_rx.await;
    }

    pub async fn hide_spinner(&self) {
        let (done_tx, done_rx) = oneshot::channel();
        let _ = self.cmd_tx.send(Command::SetSpinner {
            active: false,
            done_tx: Some(done_tx),
        });
        let _ = done_rx.await;
    }

    pub async fn prompt(&self) -> Option<String> {
        let (response_tx, response_rx) = oneshot::channel();
        let _ = self.cmd_tx.send(Command::Prompt { response_tx });
        response_rx.await.ok().flatten()
    }

    pub async fn select(&self, choices: Vec<Choice>) -> Option<Choice> {
        let (response_tx, response_rx) = oneshot::channel();
        let _ = self.cmd_tx.send(Command::Select {
            choices,
            response_tx,
        });
        response_rx.await.ok().flatten()
    }
}

impl Terminal {
    pub fn new() -> (Stream, Self) {
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let stream = Stream { cmd_tx };
        let terminal = Terminal {
            cmd_rx,
            history: Vec::new(),
            spinner: Spinner::new(),
        };
        (stream, terminal)
    }

    pub async fn run(mut self) {
        let mut stdout = io::stdout();
        // prompt_lines: how many lines the current idle prompt occupies (0 = not drawn).
        // at_bol: whether the cursor is at the start of a fresh line (no inline content pending).
        let mut at_bol = true;
        let mut spinner_tick = time::interval(Duration::from_millis(120));
        let mut prompt_lines = draw_idle_prompt(&mut stdout, &self.spinner);

        loop {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    erase_prompt(&mut stdout, prompt_lines);
                    break;
                }
                _ = spinner_tick.tick(), if self.spinner.active => {
                    self.spinner.tick();
                    redraw_idle(&mut stdout, &self.spinner, &mut prompt_lines, &mut at_bol);
                }
                Some(cmd) = self.cmd_rx.recv() => {
                    match cmd {
                        Command::Print { message, color, done_tx } => {
                            ensure_bol(&mut stdout, &mut prompt_lines, &mut at_bol);
                            let _ = print_line(&mut stdout, &message, color);
                            prompt_lines = draw_idle_prompt(&mut stdout, &self.spinner);
                            at_bol = false;
                            if let Some(tx) = done_tx { let _ = tx.send(()); }
                        }
                        Command::Write { text, color, done_tx } => {
                            // Drain consecutive fire-and-forget writes for smooth streaming.
                            // Only erase the idle prompt once, then stream directly without
                            // redrawing it — the prompt will reappear on the next Print /
                            // SetSpinner / spinner tick.
                            let mut texts: Vec<(String, Option<Color>)> = vec![(text, color)];
                            while let Ok(Command::Write { text, color, .. }) =
                                self.cmd_rx.try_recv()
                            {
                                texts.push((text, color));
                            }
                            if prompt_lines > 0 {
                                erase_prompt(&mut stdout, prompt_lines);
                                prompt_lines = 0;
                            }
                            for (t, c) in &texts {
                                at_bol = t.ends_with('\n');
                                let _ = write_inline(&mut stdout, t, *c);
                            }
                            if let Some(tx) = done_tx { let _ = tx.send(()); }
                        }
                        Command::SetSpinner { active, done_tx } => {
                            ensure_bol(&mut stdout, &mut prompt_lines, &mut at_bol);
                            self.spinner.active = active;
                            prompt_lines = draw_idle_prompt(&mut stdout, &self.spinner);
                            at_bol = false;
                            if let Some(tx) = done_tx { let _ = tx.send(()); }
                        }
                        Command::Prompt { response_tx } => {
                            ensure_bol(&mut stdout, &mut prompt_lines, &mut at_bol);
                            let result = self.read_prompt(&mut stdout);
                            prompt_lines = draw_idle_prompt(&mut stdout, &self.spinner);
                            at_bol = false;
                            let _ = response_tx.send(result.unwrap_or(None));
                        }
                        Command::Select { choices, response_tx } => {
                            ensure_bol(&mut stdout, &mut prompt_lines, &mut at_bol);
                            let result = self.select_choice(&mut stdout, &choices);
                            prompt_lines = draw_idle_prompt(&mut stdout, &self.spinner);
                            at_bol = false;
                            let _ = response_tx.send(result.unwrap_or(None));
                        }
                    }
                }
            }
        }
    }

    fn read_prompt(&mut self, stdout: &mut io::Stdout) -> anyhow::Result<Option<String>> {
        let _raw = RawMode::new()?;
        let mut editor = PromptEditor::new(&self.history);
        let mut prompt_lines = draw_input_prompt(stdout, &self.spinner, &editor)?;
        let mut at_bol = false;

        loop {
            self.handle_waiting_output(stdout, &mut prompt_lines, &mut at_bol)?;
            if prompt_lines == 0 {
                // Output arrived; redraw input prompt below it.
                if !at_bol {
                    queue!(stdout, Print("\r\n"))?;
                    stdout.flush()?;
                }
                prompt_lines = draw_input_prompt(stdout, &self.spinner, &editor)?;
                at_bol = false;
            }

            if !event::poll(Duration::from_millis(120))? {
                self.spinner.tick();
                erase_prompt(stdout, prompt_lines);
                prompt_lines = draw_input_prompt(stdout, &self.spinner, &editor)?;
                at_bol = false;
                continue;
            }

            match event::read()? {
                Event::Key(key) if is_cancel(key) => {
                    erase_prompt(stdout, prompt_lines);
                    return Ok(None);
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Enter,
                    ..
                }) => {
                    let line = editor.buffer.clone();
                    if !line.is_empty() {
                        self.history.push(line.clone());
                    }
                    erase_prompt(stdout, prompt_lines);
                    let _ = print_line(stdout, &format!("> {line}"), None);
                    return Ok(Some(line));
                }
                Event::Key(key) => {
                    editor.handle_key(key);
                    erase_prompt(stdout, prompt_lines);
                    prompt_lines = draw_input_prompt(stdout, &self.spinner, &editor)?;
                    at_bol = false;
                }
                _ => {}
            }
        }
    }

    fn select_choice(
        &mut self,
        stdout: &mut io::Stdout,
        choices: &[Choice],
    ) -> anyhow::Result<Option<Choice>> {
        if choices.is_empty() {
            return Ok(None);
        }

        let _raw = RawMode::new()?;
        let mut pager = PagerState::new(choices.len());
        let mut prompt_lines = draw_choices(stdout, &self.spinner, choices, &pager)?;
        let mut at_bol = false;

        loop {
            self.handle_waiting_output(stdout, &mut prompt_lines, &mut at_bol)?;
            if prompt_lines == 0 {
                if !at_bol {
                    queue!(stdout, Print("\r\n"))?;
                    stdout.flush()?;
                }
                prompt_lines = draw_choices(stdout, &self.spinner, choices, &pager)?;
                at_bol = false;
            }

            if !event::poll(Duration::from_millis(120))? {
                self.spinner.tick();
                erase_prompt(stdout, prompt_lines);
                prompt_lines = draw_choices(stdout, &self.spinner, choices, &pager)?;
                at_bol = false;
                continue;
            }

            match event::read()? {
                Event::Key(key) if is_cancel(key) => {
                    erase_prompt(stdout, prompt_lines);
                    return Ok(None);
                }
                Event::Key(KeyEvent {
                    code: KeyCode::Enter,
                    ..
                }) => {
                    let selected = choices[pager.selected].clone();
                    erase_prompt(stdout, prompt_lines);
                    return Ok(Some(selected));
                }
                Event::Key(key) => {
                    pager.handle_key(key);
                    erase_prompt(stdout, prompt_lines);
                    prompt_lines = draw_choices(stdout, &self.spinner, choices, &pager)?;
                    at_bol = false;
                }
                _ => {}
            }
        }
    }

    fn handle_waiting_output(
        &mut self,
        stdout: &mut io::Stdout,
        prompt_lines: &mut usize,
        at_bol: &mut bool,
    ) -> anyhow::Result<()> {
        let mut erased = false;
        loop {
            match self.cmd_rx.try_recv() {
                Ok(Command::Print { message, color, done_tx }) => {
                    if !erased {
                        erase_prompt(stdout, *prompt_lines);
                        erased = true;
                    }
                    print_line(stdout, &message, color)?;
                    *at_bol = true;
                    if let Some(tx) = done_tx {
                        let _ = tx.send(());
                    }
                }
                Ok(Command::Write { text, color, done_tx }) => {
                    if !erased {
                        erase_prompt(stdout, *prompt_lines);
                        erased = true;
                    }
                    *at_bol = text.ends_with('\n');
                    write_inline(stdout, &text, color)?;
                    if let Some(tx) = done_tx {
                        let _ = tx.send(());
                    }
                }
                Ok(Command::SetSpinner { active, done_tx }) => {
                    self.spinner.active = active;
                    if let Some(tx) = done_tx {
                        let _ = tx.send(());
                    }
                }
                Ok(Command::Prompt { response_tx }) => {
                    let _ = response_tx.send(None);
                }
                Ok(Command::Select { response_tx, .. }) => {
                    let _ = response_tx.send(None);
                }
                Err(_) => {
                    if erased {
                        *prompt_lines = 0;
                    }
                    return Ok(());
                }
            }
        }
    }
}

// --- Render primitives ---

/// Erase `n` prompt lines. Cursor ends at column 0 of the topmost cleared line.
fn erase_prompt(stdout: &mut io::Stdout, n: usize) {
    if n == 0 {
        return;
    }
    let _ = queue!(stdout, cursor::MoveToColumn(0), Clear(ClearType::CurrentLine));
    for _ in 1..n {
        let _ = queue!(
            stdout,
            cursor::MoveToPreviousLine(1),
            Clear(ClearType::CurrentLine)
        );
    }
    let _ = stdout.flush();
}

/// Ensure cursor is at the start of a fresh line before drawing a prompt.
/// Erases the idle prompt if drawn, or emits `\r\n` when mid-line after inline writes.
fn ensure_bol(stdout: &mut io::Stdout, prompt_lines: &mut usize, at_bol: &mut bool) {
    if *prompt_lines > 0 {
        erase_prompt(stdout, *prompt_lines);
        *prompt_lines = 0;
    } else if !*at_bol {
        let _ = queue!(stdout, Print("\r\n"));
        let _ = stdout.flush();
        *at_bol = true;
    }
}

/// Redraw the idle prompt after a spinner tick.
fn redraw_idle(
    stdout: &mut io::Stdout,
    spinner: &Spinner,
    prompt_lines: &mut usize,
    at_bol: &mut bool,
) {
    if *prompt_lines > 0 {
        erase_prompt(stdout, *prompt_lines);
    } else if !*at_bol {
        let _ = queue!(stdout, Print("\r\n"));
        let _ = stdout.flush();
    }
    *prompt_lines = draw_idle_prompt(stdout, spinner);
    *at_bol = false;
}

/// Draw the idle prompt (`> `, with optional spinner line above). Returns lines drawn.
/// Caller must ensure cursor is at column 0 of a fresh line before calling.
fn draw_idle_prompt(stdout: &mut io::Stdout, spinner: &Spinner) -> usize {
    let lines = if spinner.active {
        let _ = queue!(stdout, Print(spinner.frame()), Print(" waiting\r\n"));
        2
    } else {
        1
    };
    let _ = queue!(stdout, Print("> "));
    let _ = stdout.flush();
    lines
}

/// Draw the interactive input prompt. Returns lines drawn.
fn draw_input_prompt(
    stdout: &mut io::Stdout,
    spinner: &Spinner,
    editor: &PromptEditor,
) -> anyhow::Result<usize> {
    let lines = if spinner.active {
        queue!(stdout, Print(spinner.frame()), Print(" waiting\r\n"))?;
        2
    } else {
        1
    };
    queue!(
        stdout,
        Print("> "),
        Print(&editor.buffer),
        cursor::MoveToColumn((2 + editor.cursor) as u16)
    )?;
    stdout.flush()?;
    Ok(lines)
}

/// Draw the choice selector. Returns lines drawn.
fn draw_choices(
    stdout: &mut io::Stdout,
    spinner: &Spinner,
    choices: &[Choice],
    pager: &PagerState,
) -> anyhow::Result<usize> {
    let mut lines = if spinner.active {
        queue!(stdout, Print(spinner.frame()), Print(" waiting\r\n"))?;
        1
    } else {
        0
    };

    queue!(stdout, Print("> "))?;
    lines += 1;

    for idx in pager.visible_range() {
        let choice = &choices[idx];
        queue!(stdout, Print("\r\n"), Clear(ClearType::CurrentLine))?;
        lines += 1;
        if idx == pager.selected {
            queue!(stdout, SetForegroundColor(Color::Cyan), Print("> "), ResetColor)?;
        } else {
            queue!(stdout, Print("  "))?;
        }
        queue!(stdout, Print(&choice.display))?;
        if let Some(description) = &choice.description {
            queue!(
                stdout,
                SetForegroundColor(Color::DarkGrey),
                Print("  "),
                Print(description),
                ResetColor
            )?;
        }
    }

    if pager.len > pager.max_visible {
        let end = pager.visible_range().end;
        queue!(
            stdout,
            Print("\r\n"),
            Clear(ClearType::CurrentLine),
            SetForegroundColor(Color::DarkGrey),
            Print(format!(
                "rows {} to {} of {}",
                pager.offset + 1,
                end,
                pager.len
            )),
            ResetColor
        )?;
        lines += 1;
    }

    if lines > 1 {
        queue!(stdout, cursor::MoveUp((lines - 1) as u16))?;
    }
    queue!(stdout, cursor::MoveToColumn(2))?;
    stdout.flush()?;
    Ok(lines)
}

fn print_line(
    stdout: &mut io::Stdout,
    message: &str,
    color: Option<Color>,
) -> anyhow::Result<()> {
    queue_colored(stdout, message, color)?;
    queue!(stdout, Print("\r\n"))?;
    stdout.flush()?;
    Ok(())
}

fn write_inline(
    stdout: &mut io::Stdout,
    text: &str,
    color: Option<Color>,
) -> anyhow::Result<()> {
    queue_colored(stdout, text, color)?;
    stdout.flush()?;
    Ok(())
}

fn queue_colored(stdout: &mut io::Stdout, text: &str, color: Option<Color>) -> anyhow::Result<()> {
    if let Some(color) = color {
        queue!(stdout, SetForegroundColor(color), Print(text), ResetColor)?;
    } else {
        queue!(stdout, Print(text))?;
    }
    Ok(())
}

// --- Input helpers ---

fn is_cancel(key: KeyEvent) -> bool {
    matches!(
        key,
        KeyEvent { code: KeyCode::Esc, .. }
            | KeyEvent {
                code: KeyCode::Char('c'),
                modifiers: KeyModifiers::CONTROL,
                ..
            }
            | KeyEvent {
                code: KeyCode::Char('d'),
                modifiers: KeyModifiers::CONTROL,
                ..
            }
    )
}

struct RawMode;

impl RawMode {
    fn new() -> anyhow::Result<Self> {
        terminal::enable_raw_mode()?;
        Ok(Self)
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
    }
}

#[derive(Debug)]
struct Spinner {
    active: bool,
    frame: usize,
}

impl Spinner {
    fn new() -> Self {
        Self {
            active: false,
            frame: 0,
        }
    }

    fn tick(&mut self) {
        if self.active {
            self.frame = (self.frame + 1) % SPINNER_FRAMES.len();
        }
    }

    fn frame(&self) -> &'static str {
        SPINNER_FRAMES[self.frame]
    }
}

const SPINNER_FRAMES: [&str; 4] = ["|", "/", "-", "\\"];

#[derive(Debug)]
struct PromptEditor {
    buffer: String,
    cursor: usize,
    history: Vec<String>,
    history_index: Option<usize>,
    draft: String,
}

impl PromptEditor {
    fn new(history: &[String]) -> Self {
        Self {
            buffer: String::new(),
            cursor: 0,
            history: history.to_vec(),
            history_index: None,
            draft: String::new(),
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char(ch)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                self.insert(ch);
            }
            KeyCode::Backspace => self.backspace(),
            KeyCode::Delete => self.delete(),
            KeyCode::Left => self.move_left(),
            KeyCode::Right => self.move_right(),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.len(),
            KeyCode::Up => self.history_prev(),
            KeyCode::Down => self.history_next(),
            _ => {}
        }
    }

    fn insert(&mut self, ch: char) {
        let byte = self.byte_index(self.cursor);
        self.buffer.insert(byte, ch);
        self.cursor += 1;
        self.history_index = None;
    }

    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let start = self.byte_index(self.cursor - 1);
        let end = self.byte_index(self.cursor);
        self.buffer.replace_range(start..end, "");
        self.cursor -= 1;
        self.history_index = None;
    }

    fn delete(&mut self) {
        if self.cursor == self.len() {
            return;
        }
        let start = self.byte_index(self.cursor);
        let end = self.byte_index(self.cursor + 1);
        self.buffer.replace_range(start..end, "");
        self.history_index = None;
    }

    fn move_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    fn move_right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.len());
    }

    fn history_prev(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let next = match self.history_index {
            Some(0) => 0,
            Some(idx) => idx - 1,
            None => {
                self.draft = self.buffer.clone();
                self.history.len() - 1
            }
        };
        self.apply_history(next);
    }

    fn history_next(&mut self) {
        let Some(idx) = self.history_index else {
            return;
        };
        if idx + 1 == self.history.len() {
            self.history_index = None;
            self.buffer = self.draft.clone();
            self.cursor = self.len();
        } else {
            self.apply_history(idx + 1);
        }
    }

    fn apply_history(&mut self, idx: usize) {
        self.history_index = Some(idx);
        self.buffer = self.history[idx].clone();
        self.cursor = self.len();
    }

    fn len(&self) -> usize {
        self.buffer.chars().count()
    }

    fn byte_index(&self, char_idx: usize) -> usize {
        self.buffer
            .char_indices()
            .nth(char_idx)
            .map(|(idx, _)| idx)
            .unwrap_or(self.buffer.len())
    }
}

#[derive(Debug)]
struct PagerState {
    selected: usize,
    offset: usize,
    len: usize,
    max_visible: usize,
}

impl PagerState {
    fn new(len: usize) -> Self {
        Self {
            selected: 0,
            offset: 0,
            len,
            max_visible: 8,
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Down | KeyCode::Tab => self.select_next(),
            KeyCode::Up | KeyCode::BackTab => self.select_prev(),
            KeyCode::PageDown => self.select_page_down(),
            KeyCode::PageUp => self.select_page_up(),
            KeyCode::Home => self.select(0),
            KeyCode::End => self.select(self.len.saturating_sub(1)),
            _ => {}
        }
    }

    fn select_next(&mut self) {
        self.select((self.selected + 1) % self.len);
    }

    fn select_prev(&mut self) {
        self.select(self.selected.checked_sub(1).unwrap_or(self.len - 1));
    }

    fn select_page_down(&mut self) {
        self.select((self.selected + self.max_visible).min(self.len - 1));
    }

    fn select_page_up(&mut self) {
        self.select(self.selected.saturating_sub(self.max_visible));
    }

    fn select(&mut self, selected: usize) {
        self.selected = selected.min(self.len - 1);
        if self.selected < self.offset {
            self.offset = self.selected;
        } else if self.selected >= self.offset + self.max_visible {
            self.offset = self.selected + 1 - self.max_visible;
        }
    }

    fn visible_range(&self) -> std::ops::Range<usize> {
        self.offset..(self.offset + self.max_visible).min(self.len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn editor_inserts_at_cursor_and_moves_home_end() {
        let mut editor = PromptEditor::new(&[]);
        editor.handle_key(key(KeyCode::Char('a')));
        editor.handle_key(key(KeyCode::Char('c')));
        editor.handle_key(key(KeyCode::Left));
        editor.handle_key(key(KeyCode::Char('b')));
        assert_eq!(editor.buffer, "abc");
        assert_eq!(editor.cursor, 2);

        editor.handle_key(key(KeyCode::Home));
        assert_eq!(editor.cursor, 0);
        editor.handle_key(key(KeyCode::End));
        assert_eq!(editor.cursor, 3);
    }

    #[test]
    fn editor_deletes_by_char_index() {
        let mut editor = PromptEditor::new(&[]);
        editor.handle_key(key(KeyCode::Char('a')));
        editor.handle_key(key(KeyCode::Char('ж')));
        editor.handle_key(key(KeyCode::Char('b')));
        editor.handle_key(key(KeyCode::Left));
        editor.handle_key(key(KeyCode::Backspace));
        assert_eq!(editor.buffer, "ab");
        assert_eq!(editor.cursor, 1);

        editor.handle_key(key(KeyCode::Delete));
        assert_eq!(editor.buffer, "a");
    }

    #[test]
    fn editor_navigates_history_and_restores_draft() {
        let history = vec!["first".to_string(), "second".to_string()];
        let mut editor = PromptEditor::new(&history);
        editor.handle_key(key(KeyCode::Char('d')));
        editor.handle_key(key(KeyCode::Char('r')));

        editor.handle_key(key(KeyCode::Up));
        assert_eq!(editor.buffer, "second");
        editor.handle_key(key(KeyCode::Up));
        assert_eq!(editor.buffer, "first");
        editor.handle_key(key(KeyCode::Down));
        assert_eq!(editor.buffer, "second");
        editor.handle_key(key(KeyCode::Down));
        assert_eq!(editor.buffer, "dr");
    }

    #[test]
    fn pager_wraps_and_keeps_selection_visible() {
        let mut pager = PagerState::new(10);
        for _ in 0..8 {
            pager.handle_key(key(KeyCode::Down));
        }
        assert_eq!(pager.selected, 8);
        assert_eq!(pager.offset, 1);

        pager.handle_key(key(KeyCode::Down));
        pager.handle_key(key(KeyCode::Down));
        assert_eq!(pager.selected, 0);
        assert_eq!(pager.offset, 0);

        pager.handle_key(key(KeyCode::Up));
        assert_eq!(pager.selected, 9);
        assert_eq!(pager.offset, 2);
    }

    #[test]
    fn pager_page_and_home_end_navigation() {
        let mut pager = PagerState::new(30);
        pager.handle_key(key(KeyCode::PageDown));
        assert_eq!(pager.selected, 8);
        assert_eq!(pager.offset, 1);

        pager.handle_key(key(KeyCode::End));
        assert_eq!(pager.selected, 29);
        assert_eq!(pager.offset, 22);

        pager.handle_key(key(KeyCode::PageUp));
        assert_eq!(pager.selected, 21);
        assert_eq!(pager.offset, 21);

        pager.handle_key(key(KeyCode::Home));
        assert_eq!(pager.selected, 0);
        assert_eq!(pager.offset, 0);
    }

    #[test]
    fn spinner_ticks_only_when_active() {
        let mut spinner = Spinner::new();
        spinner.tick();
        assert_eq!(spinner.frame(), "|");

        spinner.active = true;
        spinner.tick();
        assert_eq!(spinner.frame(), "/");
    }
}
