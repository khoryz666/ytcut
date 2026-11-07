//! Text helpers: wrapping, cropping, truncation, and argument splitting.

/// Truncate a string to `max_len` characters, appending an ellipsis when cut.
pub fn truncate_str(s: &str, max_len: usize) -> String {
    if s.chars().count() > max_len {
        let truncated: String = s.chars().take(max_len.saturating_sub(1)).collect();
        format!("{truncated}…")
    } else {
        s.to_string()
    }
}

/// Wrap text into lines no wider than `max` characters, breaking at word
/// boundaries (and hard-breaking any single word longer than the limit).
pub fn wrap_word(text: &str, max: usize) -> Vec<String> {
    let max = max.max(1);
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();

    for word in text.split_whitespace() {
        if !current.is_empty() && current.chars().count() + 1 + word.chars().count() > max {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        for ch in word.chars() {
            if current.chars().count() >= max {
                lines.push(std::mem::take(&mut current));
            }
            current.push(ch);
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// Truncate to fit `max` characters, cutting at a word boundary with an ellipsis.
pub fn crop_word(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    if max <= 1 {
        return "…".to_string();
    }
    let mut truncated: String = text.chars().take(max - 1).collect();
    if let Some(space) = truncated.rfind(' ') {
        truncated.truncate(space);
    }
    truncated.push('…');
    truncated
}

/// Split a command line into arguments, honoring quotes and backslash escapes.
pub fn shlex_split(input: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quote: Option<char> = None;
    let mut escape = false;

    for c in input.chars() {
        if escape {
            current.push(c);
            escape = false;
        } else if c == '\\' {
            escape = true;
        } else if let Some(q) = in_quote {
            if c == q {
                in_quote = None;
            } else {
                current.push(c);
            }
        } else if c == '"' || c == '\'' {
            in_quote = Some(c);
        } else if c.is_whitespace() {
            if !current.is_empty() {
                args.push(std::mem::take(&mut current));
            }
        } else {
            current.push(c);
        }
    }
    if !current.is_empty() {
        args.push(current);
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shlex_split_handles_quotes_and_flags() {
        assert_eq!(
            shlex_split("https://youtu.be/xyz --name \"My Song\" --start 1:00 --end 2:00"),
            vec![
                "https://youtu.be/xyz",
                "--name",
                "My Song",
                "--start",
                "1:00",
                "--end",
                "2:00"
            ]
        );
        assert_eq!(
            shlex_split("https://youtu.be/xyz 'song name' 10 20"),
            vec!["https://youtu.be/xyz", "song name", "10", "20"]
        );
    }

    #[test]
    fn wrap_word_wraps_at_boundaries_and_breaks_long_words() {
        assert_eq!(wrap_word("hello world", 20), vec!["hello world"]);
        assert_eq!(wrap_word("hello world", 8), vec!["hello", "world"]);
        assert_eq!(
            wrap_word("a very long title here", 10),
            vec!["a very", "long title", "here"]
        );
        assert_eq!(
            wrap_word("supercalifragilistic", 5),
            vec!["super", "calif", "ragil", "istic"]
        );
        assert_eq!(wrap_word("", 10), vec![""]);
    }

    #[test]
    fn crop_word_cuts_at_word_boundary() {
        assert_eq!(crop_word("short", 10), "short");
        assert_eq!(crop_word("hello wonderful world", 12), "hello…");
        assert_eq!(crop_word("abcdefghij", 5), "abcd…");
        assert_eq!(crop_word("oneword", 1), "…");
    }
}
