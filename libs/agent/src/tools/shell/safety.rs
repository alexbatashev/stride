use std::collections::HashSet;

/// Conservative read-only check shared by shell backends.
///
/// Returns true only when the command line consists exclusively of commands in
/// `safe`, joined by sequencing or pipe operators, with no output redirection,
/// command substitution, or shell control flow. Any parsing uncertainty (e.g.
/// unbalanced quotes) yields false so the caller falls back to asking for
/// approval.
pub fn command_is_read_only(command: &str, safe: &HashSet<&str>) -> bool {
    let Some(segments) = split_segments(command) else {
        return false;
    };
    let mut saw_command = false;
    for seg in &segments {
        match segment_command(seg, safe) {
            SegmentVerdict::Empty => {}
            SegmentVerdict::Safe => saw_command = true,
            SegmentVerdict::Unsafe => return false,
        }
    }
    saw_command
}

enum SegmentVerdict {
    Empty,
    Safe,
    Unsafe,
}

const CONTROL_KEYWORDS: &[&str] = &[
    "if", "then", "elif", "else", "fi", "for", "while", "until", "do", "done", "case", "esac",
    "function", "select", "{", "}", "[[", "((",
];

fn segment_command(seg: &str, safe: &HashSet<&str>) -> SegmentVerdict {
    let trimmed = seg.trim();
    if trimmed.is_empty() {
        return SegmentVerdict::Empty;
    }
    // Output redirection or command substitution escapes our analysis.
    if trimmed.contains('>') || trimmed.contains('`') || trimmed.contains("$(") {
        return SegmentVerdict::Unsafe;
    }

    let words = split_words(trimmed);
    // Skip leading `VAR=value` environment assignments.
    let cmd = words.iter().find(|w| !is_assignment(w));
    let Some(cmd) = cmd else {
        // Only assignments: a no-op write to the shell environment, treat as safe.
        return SegmentVerdict::Safe;
    };

    let name = cmd.rsplit('/').next().unwrap_or(cmd);
    if CONTROL_KEYWORDS.contains(&name) {
        return SegmentVerdict::Unsafe;
    }
    if safe.contains(name) {
        SegmentVerdict::Safe
    } else {
        SegmentVerdict::Unsafe
    }
}

fn is_assignment(word: &str) -> bool {
    let Some((name, _)) = word.split_once('=') else {
        return false;
    };
    !name.is_empty()
        && name
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Splits a command line into segments at unquoted `;`, `|`, `&` and newlines.
/// Returns None on unbalanced quotes.
fn split_segments(s: &str) -> Option<Vec<String>> {
    let mut segments = Vec::new();
    let mut cur = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                cur.push(c);
                loop {
                    match chars.next() {
                        Some('\'') => {
                            cur.push('\'');
                            break;
                        }
                        Some(ch) => cur.push(ch),
                        None => return None,
                    }
                }
            }
            '"' => {
                cur.push(c);
                loop {
                    match chars.next() {
                        Some('"') => {
                            cur.push('"');
                            break;
                        }
                        Some('\\') => {
                            cur.push('\\');
                            match chars.next() {
                                Some(n) => cur.push(n),
                                None => return None,
                            }
                        }
                        Some(ch) => cur.push(ch),
                        None => return None,
                    }
                }
            }
            '\\' => {
                cur.push(c);
                match chars.next() {
                    Some(n) => cur.push(n),
                    None => return None,
                }
            }
            ';' | '\n' | '|' | '&' => segments.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    segments.push(cur);
    Some(segments)
}

/// Whitespace-splits a segment into words, resolving quotes away.
fn split_words(seg: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut in_word = false;
    let mut chars = seg.chars();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut cur));
                    in_word = false;
                }
            }
            '\'' => {
                in_word = true;
                for ch in chars.by_ref() {
                    if ch == '\'' {
                        break;
                    }
                    cur.push(ch);
                }
            }
            '"' => {
                in_word = true;
                while let Some(ch) = chars.next() {
                    match ch {
                        '"' => break,
                        '\\' => {
                            if let Some(n) = chars.next() {
                                cur.push(n);
                            }
                        }
                        _ => cur.push(ch),
                    }
                }
            }
            '\\' => {
                in_word = true;
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            _ => {
                in_word = true;
                cur.push(c);
            }
        }
    }
    if in_word {
        words.push(cur);
    }
    words
}

#[cfg(test)]
mod tests {
    use super::*;

    fn safe_set() -> HashSet<&'static str> {
        ["ls", "cat", "echo", "grep", "pwd"].into_iter().collect()
    }

    #[test]
    fn plain_safe_command() {
        assert!(command_is_read_only("ls -la", &safe_set()));
    }

    #[test]
    fn unknown_command_is_unsafe() {
        assert!(!command_is_read_only("rm -rf /", &safe_set()));
    }

    #[test]
    fn pipeline_all_safe() {
        assert!(command_is_read_only("ls | grep foo | cat", &safe_set()));
    }

    #[test]
    fn pipeline_with_unsafe_part() {
        assert!(!command_is_read_only("ls | rm bar", &safe_set()));
    }

    #[test]
    fn sequence_with_unsafe_part() {
        assert!(!command_is_read_only("ls; rm bar", &safe_set()));
    }

    #[test]
    fn output_redirection_is_unsafe() {
        assert!(!command_is_read_only("echo hi > file", &safe_set()));
    }

    #[test]
    fn command_substitution_is_unsafe() {
        assert!(!command_is_read_only("echo $(rm x)", &safe_set()));
    }

    #[test]
    fn control_flow_is_unsafe() {
        assert!(!command_is_read_only(
            "for f in *; do cat $f; done",
            &safe_set()
        ));
    }

    #[test]
    fn leading_assignment_is_skipped() {
        assert!(command_is_read_only("FOO=bar ls", &safe_set()));
        assert!(!command_is_read_only("FOO=bar rm x", &safe_set()));
    }

    #[test]
    fn unbalanced_quote_is_unsafe() {
        assert!(!command_is_read_only("echo 'unterminated", &safe_set()));
    }

    #[test]
    fn empty_is_unsafe() {
        assert!(!command_is_read_only("   ", &safe_set()));
    }
}
