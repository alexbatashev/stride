//! A small bash subset interpreted directly over the VFS.
//!
//! Supported: variable assignment and `$`/`${}` expansion, single/double
//! quoting, command sequences (`;`, newline, `&&`, `||`), pipelines (`|`),
//! output redirection (`>`, `>>`, `<`), and the `if`/`for`/`while` compound
//! commands. No word splitting after expansion, no globbing, no command
//! substitution, no subshells.

use std::collections::HashMap;
use std::future::Future;
use std::mem;
use std::pin::Pin;

use crate::vfs::{EntryKind, MountedVfs};

const MAX_LOOP_ITERATIONS: usize = 100_000;

pub(super) async fn run(
    fs: &MountedVfs,
    command: &str,
    cwd: &str,
) -> Result<(String, String, i32), String> {
    let tokens = lex(command)?;
    let program = Parser::new(tokens).parse_program()?;

    let mut shell = Shell {
        fs: fs.clone(),
        vars: HashMap::new(),
        cwd: normalize("", cwd),
        status: 0,
        stdout: String::new(),
        stderr: String::new(),
    };
    shell.eval(&program).await;
    Ok((shell.stdout, shell.stderr, shell.status))
}

// ----------------------------------------------------------------------------
// Lexer
// ----------------------------------------------------------------------------

#[derive(Clone)]
enum Part {
    Lit(String),
    Var(String),
}

#[derive(Clone)]
struct Word {
    parts: Vec<Part>,
    quoted: bool,
}

impl Word {
    fn keyword(&self) -> Option<&str> {
        if self.quoted || self.parts.len() != 1 {
            return None;
        }
        match &self.parts[0] {
            Part::Lit(s) => Some(s.as_str()),
            Part::Var(_) => None,
        }
    }
}

#[derive(Clone)]
enum Token {
    Word(Word),
    Semi,
    AndAnd,
    OrOr,
    Pipe,
    Less,
    Great,
    GreatGreat,
}

fn lex(input: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            ' ' | '\t' | '\r' => i += 1,
            '#' => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '\n' | ';' => {
                tokens.push(Token::Semi);
                i += 1;
            }
            '|' => {
                if chars.get(i + 1) == Some(&'|') {
                    tokens.push(Token::OrOr);
                    i += 2;
                } else {
                    tokens.push(Token::Pipe);
                    i += 1;
                }
            }
            '&' => {
                if chars.get(i + 1) == Some(&'&') {
                    tokens.push(Token::AndAnd);
                    i += 2;
                } else {
                    return Err("background execution (&) is not supported".to_string());
                }
            }
            '<' => {
                tokens.push(Token::Less);
                i += 1;
            }
            '>' => {
                if chars.get(i + 1) == Some(&'>') {
                    tokens.push(Token::GreatGreat);
                    i += 2;
                } else {
                    tokens.push(Token::Great);
                    i += 1;
                }
            }
            _ => {
                let (word, next) = lex_word(&chars, i)?;
                tokens.push(Token::Word(word));
                i = next;
            }
        }
    }
    Ok(tokens)
}

fn lex_word(chars: &[char], start: usize) -> Result<(Word, usize), String> {
    let mut parts: Vec<Part> = Vec::new();
    let mut lit = String::new();
    let mut quoted = false;
    let mut i = start;

    let flush = |lit: &mut String, parts: &mut Vec<Part>| {
        if !lit.is_empty() {
            parts.push(Part::Lit(mem::take(lit)));
        }
    };

    while i < chars.len() {
        let c = chars[i];
        match c {
            ' ' | '\t' | '\r' | '\n' | ';' | '|' | '&' | '<' | '>' | '#' => break,
            '\'' => {
                quoted = true;
                i += 1;
                let mut closed = false;
                while i < chars.len() {
                    if chars[i] == '\'' {
                        closed = true;
                        i += 1;
                        break;
                    }
                    lit.push(chars[i]);
                    i += 1;
                }
                if !closed {
                    return Err("unbalanced single quote".to_string());
                }
            }
            '"' => {
                quoted = true;
                i += 1;
                let mut closed = false;
                while i < chars.len() {
                    match chars[i] {
                        '"' => {
                            closed = true;
                            i += 1;
                            break;
                        }
                        '\\' => {
                            i += 1;
                            if let Some(&n) = chars.get(i) {
                                lit.push(n);
                                i += 1;
                            }
                        }
                        '$' => {
                            flush(&mut lit, &mut parts);
                            i = lex_var(chars, i, &mut parts);
                        }
                        ch => {
                            lit.push(ch);
                            i += 1;
                        }
                    }
                }
                if !closed {
                    return Err("unbalanced double quote".to_string());
                }
            }
            '\\' => {
                i += 1;
                if let Some(&n) = chars.get(i) {
                    lit.push(n);
                    i += 1;
                }
            }
            '$' => {
                flush(&mut lit, &mut parts);
                i = lex_var(chars, i, &mut parts);
            }
            ch => {
                lit.push(ch);
                i += 1;
            }
        }
    }
    flush(&mut lit, &mut parts);
    Ok((Word { parts, quoted }, i))
}

/// Reads a `$NAME`, `${NAME}` or `$?` starting at `chars[i] == '$'`.
fn lex_var(chars: &[char], start: usize, parts: &mut Vec<Part>) -> usize {
    let mut i = start + 1; // skip '$'
    if chars.get(i) == Some(&'{') {
        i += 1;
        let mut name = String::new();
        while i < chars.len() && chars[i] != '}' {
            name.push(chars[i]);
            i += 1;
        }
        if chars.get(i) == Some(&'}') {
            i += 1;
        }
        parts.push(Part::Var(name));
    } else if chars.get(i) == Some(&'?') {
        parts.push(Part::Var("?".to_string()));
        i += 1;
    } else {
        let mut name = String::new();
        while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
            name.push(chars[i]);
            i += 1;
        }
        if name.is_empty() {
            parts.push(Part::Lit("$".to_string()));
        } else {
            parts.push(Part::Var(name));
        }
    }
    i
}

// ----------------------------------------------------------------------------
// Parser
// ----------------------------------------------------------------------------

enum Node {
    Sequence(Vec<Node>),
    And(Box<Node>, Box<Node>),
    Or(Box<Node>, Box<Node>),
    Pipeline(Vec<Node>),
    Simple(Simple),
    If {
        branches: Vec<(Node, Node)>,
        else_branch: Option<Box<Node>>,
    },
    For {
        var: String,
        items: Vec<Word>,
        body: Box<Node>,
    },
    While {
        cond: Box<Node>,
        body: Box<Node>,
    },
}

struct Simple {
    assignments: Vec<(String, Word)>,
    words: Vec<Word>,
    redirects: Vec<Redirect>,
}

enum Redirect {
    Out(Word),
    Append(Word),
    In(Word),
}

const TERMINATORS: &[&str] = &["then", "elif", "else", "fi", "do", "done", "esac", "in"];

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn peek_keyword(&self) -> Option<&str> {
        match self.peek() {
            Some(Token::Word(w)) => w.keyword(),
            _ => None,
        }
    }

    fn skip_separators(&mut self) {
        while matches!(self.peek(), Some(Token::Semi)) {
            self.pos += 1;
        }
    }

    fn expect_keyword(&mut self, kw: &str) -> Result<(), String> {
        if self.peek_keyword() == Some(kw) {
            self.pos += 1;
            Ok(())
        } else {
            Err(format!("expected `{kw}`"))
        }
    }

    fn parse_program(&mut self) -> Result<Node, String> {
        let node = self.parse_list()?;
        self.skip_separators();
        if self.pos != self.tokens.len() {
            return Err("unexpected trailing tokens".to_string());
        }
        Ok(node)
    }

    /// Parses a sequence of `and_or` separated by `;`/newline, stopping at a
    /// terminator keyword or end of input.
    fn parse_list(&mut self) -> Result<Node, String> {
        let mut items = Vec::new();
        loop {
            self.skip_separators();
            if self.peek().is_none() {
                break;
            }
            if matches!(self.peek_keyword(), Some(k) if TERMINATORS.contains(&k)) {
                break;
            }
            items.push(self.parse_and_or()?);
            if matches!(self.peek(), Some(Token::Semi)) {
                continue;
            }
            break;
        }
        if items.is_empty() {
            return Err("empty command".to_string());
        }
        if items.len() == 1 {
            Ok(items.pop().unwrap())
        } else {
            Ok(Node::Sequence(items))
        }
    }

    fn parse_and_or(&mut self) -> Result<Node, String> {
        let mut left = self.parse_pipeline()?;
        loop {
            match self.peek() {
                Some(Token::AndAnd) => {
                    self.pos += 1;
                    self.skip_separators();
                    let right = self.parse_pipeline()?;
                    left = Node::And(Box::new(left), Box::new(right));
                }
                Some(Token::OrOr) => {
                    self.pos += 1;
                    self.skip_separators();
                    let right = self.parse_pipeline()?;
                    left = Node::Or(Box::new(left), Box::new(right));
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn parse_pipeline(&mut self) -> Result<Node, String> {
        let mut cmds = vec![self.parse_command()?];
        while matches!(self.peek(), Some(Token::Pipe)) {
            self.pos += 1;
            self.skip_separators();
            cmds.push(self.parse_command()?);
        }
        if cmds.len() == 1 {
            Ok(cmds.pop().unwrap())
        } else {
            Ok(Node::Pipeline(cmds))
        }
    }

    fn parse_command(&mut self) -> Result<Node, String> {
        match self.peek_keyword() {
            Some("if") => self.parse_if(),
            Some("for") => self.parse_for(),
            Some("while") => self.parse_while(),
            _ => self.parse_simple(),
        }
    }

    fn parse_if(&mut self) -> Result<Node, String> {
        self.expect_keyword("if")?;
        let mut branches = Vec::new();
        let cond = self.parse_list()?;
        self.expect_keyword("then")?;
        let body = self.parse_list()?;
        branches.push((cond, body));
        while self.peek_keyword() == Some("elif") {
            self.pos += 1;
            let cond = self.parse_list()?;
            self.expect_keyword("then")?;
            let body = self.parse_list()?;
            branches.push((cond, body));
        }
        let else_branch = if self.peek_keyword() == Some("else") {
            self.pos += 1;
            Some(Box::new(self.parse_list()?))
        } else {
            None
        };
        self.expect_keyword("fi")?;
        Ok(Node::If {
            branches,
            else_branch,
        })
    }

    fn parse_for(&mut self) -> Result<Node, String> {
        self.expect_keyword("for")?;
        let var = match self.peek() {
            Some(Token::Word(w)) => match w.keyword() {
                Some(name) if is_name(name) => {
                    let name = name.to_string();
                    self.pos += 1;
                    name
                }
                _ => return Err("invalid loop variable".to_string()),
            },
            _ => return Err("expected loop variable".to_string()),
        };
        let mut items = Vec::new();
        if self.peek_keyword() == Some("in") {
            self.pos += 1;
            while let Some(Token::Word(w)) = self.peek() {
                if matches!(w.keyword(), Some(k) if TERMINATORS.contains(&k)) {
                    break;
                }
                items.push(w.clone());
                self.pos += 1;
            }
        }
        self.skip_separators();
        self.expect_keyword("do")?;
        let body = self.parse_list()?;
        self.expect_keyword("done")?;
        Ok(Node::For {
            var,
            items,
            body: Box::new(body),
        })
    }

    fn parse_while(&mut self) -> Result<Node, String> {
        self.expect_keyword("while")?;
        let cond = self.parse_list()?;
        self.expect_keyword("do")?;
        let body = self.parse_list()?;
        self.expect_keyword("done")?;
        Ok(Node::While {
            cond: Box::new(cond),
            body: Box::new(body),
        })
    }

    fn parse_simple(&mut self) -> Result<Node, String> {
        let mut assignments = Vec::new();
        let mut words = Vec::new();
        let mut redirects = Vec::new();
        loop {
            match self.peek() {
                Some(Token::Word(w)) => {
                    if !words.is_empty()
                        && matches!(w.keyword(), Some(k) if TERMINATORS.contains(&k))
                    {
                        break;
                    }
                    if words.is_empty()
                        && let Some(assignment) = word_assignment(w)
                    {
                        assignments.push(assignment);
                        self.pos += 1;
                        continue;
                    }
                    words.push(w.clone());
                    self.pos += 1;
                }
                Some(Token::Great) => {
                    self.pos += 1;
                    redirects.push(Redirect::Out(self.expect_word()?));
                }
                Some(Token::GreatGreat) => {
                    self.pos += 1;
                    redirects.push(Redirect::Append(self.expect_word()?));
                }
                Some(Token::Less) => {
                    self.pos += 1;
                    redirects.push(Redirect::In(self.expect_word()?));
                }
                _ => break,
            }
        }
        if words.is_empty() && assignments.is_empty() {
            return Err("expected a command".to_string());
        }
        Ok(Node::Simple(Simple {
            assignments,
            words,
            redirects,
        }))
    }

    fn expect_word(&mut self) -> Result<Word, String> {
        match self.peek() {
            Some(Token::Word(w)) => {
                let w = w.clone();
                self.pos += 1;
                Ok(w)
            }
            _ => Err("expected a filename after redirection".to_string()),
        }
    }
}

fn word_assignment(word: &Word) -> Option<(String, Word)> {
    if word.quoted {
        return None;
    }
    let Some(Part::Lit(first)) = word.parts.first() else {
        return None;
    };
    let eq = first.find('=')?;
    let name = &first[..eq];
    if !is_name(name) {
        return None;
    }
    let mut value = vec![Part::Lit(first[eq + 1..].to_string())];
    value.extend(word.parts[1..].iter().cloned());
    Some((
        name.to_string(),
        Word {
            parts: value,
            quoted: false,
        },
    ))
}

fn is_name(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

// ----------------------------------------------------------------------------
// Evaluator
// ----------------------------------------------------------------------------

struct Shell {
    fs: MountedVfs,
    vars: HashMap<String, String>,
    cwd: String,
    status: i32,
    stdout: String,
    stderr: String,
}

type Fut<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

impl Shell {
    fn eval<'a>(&'a mut self, node: &'a Node) -> Fut<'a, ()> {
        Box::pin(async move {
            match node {
                Node::Sequence(items) => {
                    for item in items {
                        self.eval(item).await;
                    }
                }
                Node::And(a, b) => {
                    self.eval(a).await;
                    if self.status == 0 {
                        self.eval(b).await;
                    }
                }
                Node::Or(a, b) => {
                    self.eval(a).await;
                    if self.status != 0 {
                        self.eval(b).await;
                    }
                }
                Node::Pipeline(cmds) => {
                    let mut input = String::new();
                    for cmd in cmds {
                        input = self.eval_capturing(cmd, input).await;
                    }
                    self.stdout.push_str(&input);
                }
                Node::Simple(simple) => {
                    let out = self.run_simple(simple, String::new()).await;
                    self.stdout.push_str(&out);
                }
                Node::If {
                    branches,
                    else_branch,
                } => {
                    let mut taken = false;
                    for (cond, body) in branches {
                        self.eval(cond).await;
                        if self.status == 0 {
                            self.eval(body).await;
                            taken = true;
                            break;
                        }
                    }
                    if !taken && let Some(body) = else_branch {
                        self.eval(body).await;
                    }
                }
                Node::For { var, items, body } => {
                    for item in items {
                        let value = self.expand(item);
                        self.vars.insert(var.clone(), value);
                        self.eval(body).await;
                    }
                }
                Node::While { cond, body } => {
                    let mut iterations = 0;
                    loop {
                        self.eval(cond).await;
                        if self.status != 0 {
                            break;
                        }
                        self.eval(body).await;
                        iterations += 1;
                        if iterations >= MAX_LOOP_ITERATIONS {
                            self.stderr.push_str("while: iteration limit reached\n");
                            break;
                        }
                    }
                }
            }
        })
    }

    /// Evaluates `node`, returning its stdout instead of appending it. Only
    /// simple commands consume `stdin`.
    fn eval_capturing<'a>(&'a mut self, node: &'a Node, stdin: String) -> Fut<'a, String> {
        Box::pin(async move {
            match node {
                Node::Simple(simple) => self.run_simple(simple, stdin).await,
                other => {
                    let saved = mem::take(&mut self.stdout);
                    self.eval(other).await;
                    mem::replace(&mut self.stdout, saved)
                }
            }
        })
    }

    async fn run_simple(&mut self, simple: &Simple, mut stdin: String) -> String {
        if simple.words.is_empty() {
            for (name, value) in &simple.assignments {
                let value = self.expand(value);
                self.vars.insert(name.clone(), value);
            }
            self.status = 0;
            return String::new();
        }

        let argv: Vec<String> = simple.words.iter().map(|w| self.expand(w)).collect();

        for redirect in &simple.redirects {
            if let Redirect::In(w) = redirect {
                let path = self.resolve(&self.expand(w));
                match self.read_file(&path).await {
                    Some(content) => stdin = content,
                    None => return self.fail(format!("{path}: No such file")),
                }
            }
        }

        self.status = 0;
        let out = self.builtin(&argv[0], &argv[1..], stdin).await;

        for redirect in &simple.redirects {
            let (path, append) = match redirect {
                Redirect::Out(w) => (self.resolve(&self.expand(w)), false),
                Redirect::Append(w) => (self.resolve(&self.expand(w)), true),
                Redirect::In(_) => continue,
            };
            self.write_file(&path, &out, append).await;
            return String::new();
        }
        out
    }

    fn expand(&self, word: &Word) -> String {
        let mut out = String::new();
        for part in &word.parts {
            match part {
                Part::Lit(s) => out.push_str(s),
                Part::Var(name) => out.push_str(&self.var(name)),
            }
        }
        out
    }

    fn var(&self, name: &str) -> String {
        if name == "?" {
            return self.status.to_string();
        }
        self.vars.get(name).cloned().unwrap_or_default()
    }

    fn resolve(&self, path: &str) -> String {
        let base = if path.starts_with('/') { "" } else { &self.cwd };
        normalize(base, path)
    }

    // --- VFS helpers --------------------------------------------------------

    /// Records a command failure: sets a non-zero exit status and appends `msg`
    /// (plus a newline) to stderr. Returns empty stdout for convenience.
    fn fail(&mut self, msg: String) -> String {
        self.status = 1;
        self.stderr.push_str(&msg);
        self.stderr.push('\n');
        String::new()
    }

    async fn read_file(&self, path: &str) -> Option<String> {
        self.fs.read(path).await.ok()
    }

    /// Reads and concatenates `files`, or returns `stdin` when none are given.
    /// On a missing file records a `cmd: file` failure and returns None.
    async fn read_all(&mut self, files: &[String], stdin: String, cmd: &str) -> Option<String> {
        if files.is_empty() {
            return Some(stdin);
        }
        let mut out = String::new();
        for arg in files {
            let path = self.resolve(arg);
            match self.read_file(&path).await {
                Some(content) => out.push_str(&content),
                None => {
                    self.fail(format!("{cmd}: {arg}: not found"));
                    return None;
                }
            }
        }
        Some(out)
    }

    async fn write_file(&mut self, path: &str, content: &str, append: bool) {
        let content = if append {
            let existing = self.fs.read(path).await.unwrap_or_default();
            format!("{existing}{content}")
        } else {
            content.to_string()
        };
        if let Err(e) = self.fs.write(path, &content).await {
            self.fail(format!("{path}: {e}"));
        }
    }

    /// Returns Some(is_dir) when the path exists.
    async fn stat(&self, path: &str) -> Option<bool> {
        if path.is_empty() {
            return Some(true);
        }
        let (parent, name) = split_parent(path);
        let entries = self.fs.list(parent).await.ok()?;
        entries
            .into_iter()
            .find(|e| e.name == name)
            .map(|e| matches!(e.kind, EntryKind::Directory))
    }

    // --- builtins -----------------------------------------------------------

    async fn builtin(&mut self, cmd: &str, args: &[String], stdin: String) -> String {
        match cmd {
            "echo" => echo(args),
            "true" => String::new(),
            "false" => {
                self.status = 1;
                String::new()
            }
            "pwd" => format!("/{}\n", self.cwd),
            "cd" => self.cd(args).await,
            "ls" => self.ls(args).await,
            "cat" => self.read_all(args, stdin, "cat").await.unwrap_or_default(),
            "mkdir" => self.mkdir(args).await,
            "rm" => self.rm(args).await,
            "touch" => self.touch(args).await,
            "mv" => self.copy(args, true).await,
            "cp" => self.copy(args, false).await,
            "grep" => self.grep(args, stdin).await,
            "head" => self.head_tail(args, stdin, true).await,
            "tail" => self.head_tail(args, stdin, false).await,
            "wc" => self.wc(args, stdin).await,
            "test" | "[" => self.test(cmd, args).await,
            other => self.fail(format!("{other}: command not found")),
        }
    }

    async fn cd(&mut self, args: &[String]) -> String {
        let arg = args.first().map(String::as_str).unwrap_or("");
        let path = self.resolve(arg);
        if self.stat(&path).await == Some(true) {
            self.cwd = path;
            String::new()
        } else {
            self.fail(format!("cd: {arg}: no such directory"))
        }
    }

    async fn ls(&mut self, args: &[String]) -> String {
        let paths: Vec<&str> = if args.is_empty() {
            vec![""]
        } else {
            args.iter().map(String::as_str).collect()
        };
        let mut out = String::new();
        for arg in paths {
            let path = self.resolve(arg);
            match self.stat(&path).await {
                Some(true) => {
                    for e in self.fs.list(&path).await.unwrap_or_default() {
                        out.push_str(&e.name);
                        out.push('\n');
                    }
                }
                Some(false) => {
                    out.push_str(arg);
                    out.push('\n');
                }
                None => {
                    self.fail(format!("ls: {arg}: not found"));
                }
            }
        }
        out
    }

    async fn mkdir(&mut self, args: &[String]) -> String {
        let (flags, paths) = split_flags(args);
        for arg in &paths {
            let path = self.resolve(arg);
            if !flags.contains('p') && self.stat(&path).await.is_some() {
                self.fail(format!("mkdir: {arg}: exists"));
            } else if let Err(e) = self.fs.create_dir(&path).await {
                self.fail(format!("mkdir: {e}"));
            }
        }
        String::new()
    }

    async fn rm(&mut self, args: &[String]) -> String {
        let (flags, paths) = split_flags(args);
        let recursive = flags.contains('r') || flags.contains('R');
        for arg in &paths {
            let path = self.resolve(arg);
            if path.is_empty() {
                self.fail("rm: refusing to remove root".to_string());
            } else if let Some(is_dir) = self.stat(&path).await {
                if is_dir && !recursive {
                    self.fail(format!("rm: {arg}: is a directory"));
                } else if let Err(e) = self.fs.delete(&path).await {
                    self.fail(format!("rm: {e}"));
                }
            } else if !flags.contains('f') {
                self.fail(format!("rm: {arg}: not found"));
            }
        }
        String::new()
    }

    async fn touch(&mut self, args: &[String]) -> String {
        for arg in args {
            let path = self.resolve(arg);
            if self.stat(&path).await.is_none()
                && let Err(e) = self.fs.write(&path, "").await
            {
                self.fail(format!("touch: {e}"));
            }
        }
        String::new()
    }

    async fn copy(&mut self, args: &[String], remove_src: bool) -> String {
        let name = if remove_src { "mv" } else { "cp" };
        let ops: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();
        if ops.len() != 2 {
            return self.fail(format!("{name}: need source and destination"));
        }
        let src = self.resolve(ops[0]);
        if self.stat(&src).await != Some(false) {
            return self.fail(format!("{name}: {}: not a file", ops[0]));
        }
        let mut dst = self.resolve(ops[1]);
        if self.stat(&dst).await == Some(true) {
            dst = normalize(&dst, basename(&src));
        }
        let Some(content) = self.read_file(&src).await else {
            return self.fail(format!("{name}: {}: read error", ops[0]));
        };
        if let Err(e) = self.fs.write(&dst, &content).await {
            return self.fail(format!("{name}: {e}"));
        }
        if remove_src && let Err(e) = self.fs.delete(&src).await {
            return self.fail(format!("{name}: {e}"));
        }
        String::new()
    }

    async fn grep(&mut self, args: &[String], stdin: String) -> String {
        let mut ignore = false;
        let mut invert = false;
        let mut number = false;
        let mut rest = Vec::new();
        for arg in args {
            if rest.is_empty() && arg.starts_with('-') && arg.len() > 1 {
                for f in arg[1..].chars() {
                    match f {
                        'i' => ignore = true,
                        'v' => invert = true,
                        'n' => number = true,
                        _ => {}
                    }
                }
            } else {
                rest.push(arg.clone());
            }
        }
        if rest.is_empty() {
            return self.fail("grep: missing pattern".to_string());
        }
        let pattern = rest.remove(0);
        let needle = if ignore {
            pattern.to_lowercase()
        } else {
            pattern
        };
        let multi = rest.len() > 1;

        let mut sources: Vec<(String, String)> = Vec::new();
        if rest.is_empty() {
            sources.push((String::new(), stdin));
        } else {
            for arg in &rest {
                let path = self.resolve(arg);
                match self.read_file(&path).await {
                    Some(content) => sources.push((arg.clone(), content)),
                    None => return self.fail(format!("grep: {arg}: not found")),
                }
            }
        }

        let mut out = String::new();
        let mut matched = false;
        for (name, content) in sources {
            for (idx, line) in content.lines().enumerate() {
                let hay = if ignore {
                    line.to_lowercase()
                } else {
                    line.to_string()
                };
                if hay.contains(&needle) != invert {
                    matched = true;
                    if multi {
                        out.push_str(&name);
                        out.push(':');
                    }
                    if number {
                        out.push_str(&format!("{}:", idx + 1));
                    }
                    out.push_str(line);
                    out.push('\n');
                }
            }
        }
        self.status = if matched { 0 } else { 1 };
        out
    }

    async fn head_tail(&mut self, args: &[String], stdin: String, head: bool) -> String {
        let (count, files) = parse_count(args, 10);
        let cmd = if head { "head" } else { "tail" };
        let Some(content) = self.read_all(&files, stdin, cmd).await else {
            return String::new();
        };
        let lines: Vec<&str> = content.lines().collect();
        let selected = if head {
            &lines[..count.min(lines.len())]
        } else {
            &lines[lines.len().saturating_sub(count)..]
        };
        let mut out = selected.join("\n");
        if !out.is_empty() {
            out.push('\n');
        }
        out
    }

    async fn wc(&mut self, args: &[String], stdin: String) -> String {
        let (flags, files) = split_flags(args);
        let Some(content) = self.read_all(&files, stdin, "wc").await else {
            return String::new();
        };
        let lines = content.lines().count();
        let words = content.split_whitespace().count();
        let bytes = content.len();
        if flags.contains('l') {
            format!("{lines}\n")
        } else if flags.contains('w') {
            format!("{words}\n")
        } else if flags.contains('c') {
            format!("{bytes}\n")
        } else {
            format!("{lines} {words} {bytes}\n")
        }
    }

    async fn test(&mut self, cmd: &str, args: &[String]) -> String {
        let mut args = args.to_vec();
        if cmd == "[" {
            if args.last().map(String::as_str) != Some("]") {
                return self.fail("[: missing `]'".to_string());
            }
            args.pop();
        }
        let result = match args.as_slice() {
            [] => false,
            [a] => !a.is_empty(),
            [op, a] => match op.as_str() {
                "-z" => a.is_empty(),
                "-n" => !a.is_empty(),
                "-e" => self.stat(&self.resolve(a)).await.is_some(),
                "-f" => self.stat(&self.resolve(a)).await == Some(false),
                "-d" => self.stat(&self.resolve(a)).await == Some(true),
                _ => return self.fail(format!("test: {op}: unary operator expected")),
            },
            [a, op, b] => match op.as_str() {
                "=" | "==" => a == b,
                "!=" => a != b,
                "-eq" => int_cmp(a, b, |x, y| x == y),
                "-ne" => int_cmp(a, b, |x, y| x != y),
                "-lt" => int_cmp(a, b, |x, y| x < y),
                "-le" => int_cmp(a, b, |x, y| x <= y),
                "-gt" => int_cmp(a, b, |x, y| x > y),
                "-ge" => int_cmp(a, b, |x, y| x >= y),
                _ => return self.fail(format!("test: {op}: binary operator expected")),
            },
            _ => return self.fail("test: too many arguments".to_string()),
        };
        self.status = if result { 0 } else { 1 };
        String::new()
    }
}

// ----------------------------------------------------------------------------
// Helpers
// ----------------------------------------------------------------------------

fn echo(args: &[String]) -> String {
    let (newline, rest) = match args.first() {
        Some(flag) if flag == "-n" => (false, &args[1..]),
        _ => (true, args),
    };
    let mut out = rest.join(" ");
    if newline {
        out.push('\n');
    }
    out
}

/// Collects single-letter flags from leading `-x` arguments, returning the
/// flag characters and the remaining operands.
fn split_flags(args: &[String]) -> (String, Vec<String>) {
    let mut flags = String::new();
    let mut rest = Vec::new();
    for arg in args {
        if rest.is_empty() && arg.starts_with('-') && arg.len() > 1 {
            flags.push_str(&arg[1..]);
        } else {
            rest.push(arg.clone());
        }
    }
    (flags, rest)
}

/// Parses an optional `-n N` (or `-N`) count, returning it and the remaining files.
fn parse_count(args: &[String], default: usize) -> (usize, Vec<String>) {
    let mut count = default;
    let mut files = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "-n" {
            count = args
                .get(i + 1)
                .and_then(|s| s.parse().ok())
                .unwrap_or(count);
            i += 2;
        } else if let Some(n) = arg
            .strip_prefix("-n")
            .or_else(|| arg.strip_prefix('-'))
            .and_then(|s| s.parse().ok())
        {
            count = n;
            i += 1;
        } else {
            files.push(arg.clone());
            i += 1;
        }
    }
    (count, files)
}

fn int_cmp(a: &str, b: &str, op: impl Fn(i64, i64) -> bool) -> bool {
    match (a.parse::<i64>(), b.parse::<i64>()) {
        (Ok(a), Ok(b)) => op(a, b),
        _ => false,
    }
}

/// Normalizes `path` against `base`, resolving `.`/`..` into a workspace-relative
/// path with no leading slash (empty string is the root).
fn normalize(base: &str, path: &str) -> String {
    let mut segments: Vec<&str> = if path.starts_with('/') {
        Vec::new()
    } else {
        base.split('/').filter(|s| !s.is_empty()).collect()
    };
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            s => segments.push(s),
        }
    }
    segments.join("/")
}

fn split_parent(path: &str) -> (&str, &str) {
    match path.rfind('/') {
        Some(idx) => (&path[..idx], &path[idx + 1..]),
        None => ("", path),
    }
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::vfs::{AnyFileProvider, LocalFileProvider, Vfs};
    use minisql::{ConnectionPool, Value};
    use std::sync::Arc;
    use uuid::Uuid;

    async fn setup() -> MountedVfs {
        let db = ConnectionPool::new("sqlite::memory:").unwrap();
        db.initialize_database(db::get_migrations()).await.unwrap();
        let owner = Uuid::now_v7();
        db.query_with_params(
            "INSERT INTO users (id, username, password_hash) VALUES (?, ?, ?)",
            vec![
                Value::Uuid(owner),
                Value::Text("alice".to_string()),
                Value::Text("hash".to_string()),
            ],
        )
        .await
        .unwrap();
        let base = tempfile::tempdir().unwrap().keep();
        let storage = AnyFileProvider::Local(LocalFileProvider::new(base).unwrap());
        let vfs = Arc::new(Vfs::new(db, storage, 3));
        let ws = vfs
            .get_or_create_workspace(Uuid::now_v7(), None, owner)
            .await
            .unwrap();
        MountedVfs::new(vfs, ws, owner)
    }

    async fn exec(fs: &MountedVfs, cmd: &str) -> (String, String, i32) {
        run(fs, cmd, "/~workspace").await.unwrap()
    }

    #[tokio::test]
    async fn echo_and_pipe() {
        let fs = setup().await;
        let (out, _, code) = exec(&fs, "echo hello world").await;
        assert_eq!(out, "hello world\n");
        assert_eq!(code, 0);
    }

    #[tokio::test]
    async fn variables_and_expansion() {
        let fs = setup().await;
        let (out, _, _) = exec(&fs, "X=42; echo \"val=$X\"").await;
        assert_eq!(out, "val=42\n");
    }

    #[tokio::test]
    async fn redirect_and_read_back() {
        let fs = setup().await;
        exec(&fs, "echo line1 > /~workspace/a.txt").await;
        exec(&fs, "echo line2 >> /~workspace/a.txt").await;
        let (out, _, _) = exec(&fs, "cat /~workspace/a.txt").await;
        assert_eq!(out, "line1\nline2\n");
    }

    #[tokio::test]
    async fn writes_outside_workspace_are_rejected() {
        let fs = setup().await;
        let (_, err, code) = exec(&fs, "echo hi > /global.txt").await;
        assert_ne!(code, 0);
        assert!(err.contains("read-only"));
    }

    #[tokio::test]
    async fn pipe_to_grep() {
        let fs = setup().await;
        exec(&fs, "echo foo > f; echo bar >> f").await;
        let (out, _, code) = exec(&fs, "cat f | grep foo").await;
        assert_eq!(out, "foo\n");
        assert_eq!(code, 0);
    }

    #[tokio::test]
    async fn if_branch() {
        let fs = setup().await;
        exec(&fs, "echo hi > exists.txt").await;
        let (out, _, _) = exec(
            &fs,
            "if test -f exists.txt; then echo yes; else echo no; fi",
        )
        .await;
        assert_eq!(out, "yes\n");
    }

    #[tokio::test]
    async fn for_loop() {
        let fs = setup().await;
        let (out, _, _) = exec(&fs, "for x in a b c; do echo $x; done").await;
        assert_eq!(out, "a\nb\nc\n");
    }

    #[tokio::test]
    async fn while_loop_terminates_on_state_change() {
        let fs = setup().await;
        let (out, _, _) = exec(
            &fs,
            "echo x > flag; while test -f flag; do echo run; rm flag; done",
        )
        .await;
        assert_eq!(out, "run\n");
    }

    #[tokio::test]
    async fn mkdir_cd_and_relative_paths() {
        let fs = setup().await;
        exec(&fs, "mkdir -p a/b").await;
        exec(&fs, "echo deep > a/b/c.txt").await;
        let (out, _, _) = exec(&fs, "cd a/b; cat c.txt").await;
        assert_eq!(out, "deep\n");
    }

    #[tokio::test]
    async fn rm_and_logical_operators() {
        let fs = setup().await;
        exec(&fs, "echo x > gone.txt").await;
        let (_, _, code) = exec(&fs, "rm gone.txt && echo removed").await;
        assert_eq!(code, 0);
        let (_, _, code) = exec(&fs, "cat gone.txt").await;
        assert_eq!(code, 1);
    }

    #[tokio::test]
    async fn mv_and_cp() {
        let fs = setup().await;
        exec(&fs, "echo data > src.txt").await;
        exec(&fs, "cp src.txt copy.txt").await;
        exec(&fs, "mv src.txt moved.txt").await;
        let (out, _, _) = exec(&fs, "cat copy.txt moved.txt").await;
        assert_eq!(out, "data\ndata\n");
        let (_, _, code) = exec(&fs, "cat src.txt").await;
        assert_eq!(code, 1);
    }

    #[tokio::test]
    async fn wc_and_head_tail() {
        let fs = setup().await;
        exec(&fs, "echo a > f; echo b >> f; echo c >> f").await;
        let (out, _, _) = exec(&fs, "wc -l f").await;
        assert_eq!(out, "3\n");
        let (out, _, _) = exec(&fs, "head -n 1 f").await;
        assert_eq!(out, "a\n");
        let (out, _, _) = exec(&fs, "tail -n 1 f").await;
        assert_eq!(out, "c\n");
    }

    #[tokio::test]
    async fn parse_error_is_reported() {
        let fs = setup().await;
        let err = run(&fs, "if true; then echo hi", "/~workspace").await;
        assert!(err.is_err());
    }
}
