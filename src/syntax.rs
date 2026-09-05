pub fn detect_filetype(path: &str) -> &'static str {
    if path.ends_with(".sh") || path.ends_with(".bash") {
        "bash"
    } else if path == "Makefile" || path == "makefile" || path.ends_with(".mk") {
        "make"
    } else {
        "plain"
    }
}

fn is_ws(c: char) -> bool {
    matches!(c, ' ' | '\t')
}

fn leading_ws(line: &str) -> (usize, &str) {
    let n = line.chars().take_while(|c| is_ws(*c)).count();
    (n, &line[n..])
}

pub fn highlight_line(ft: &str, line: &str) -> String {
    if ft == "plain" {
        return line.to_string();
    }

    let (ws, rest) = leading_ws(line);
    if rest.starts_with('#') {
        let (lead, com) = line.split_at(ws);
        return format!("{}\x1b[90m{}\x1b[0m", lead, com);
    }

    if ft == "make" {
        let n = run_while(rest, make_char);
        if n > 0 && rest[n..].starts_with(':') {
            let (name, after) = rest.split_at(n);
            return format!("\x1b[33;1m{}\x1b[0m:{}", name, &after[1..]);
        }
        let n = run_while(rest, |c| c.is_ascii_alphanumeric() || c == '_');
        if n > 0 {
            let after = &rest[n..];
            let s = after.chars().take_while(|c| is_ws(*c)).count();
            if after[s..].starts_with('=') {
                let (name, tail) = rest.split_at(n);
                return format!("\x1b[36m{}\x1b[0m{}", name, tail);
            }
        }
    } else if ft == "bash" {
        let keywords: [&'static str; 14] = [
            "if", "then", "else", "elif", "fi", "for", "while", "do", "done", "case", "esac",
            "function", "return", "exit",
        ];
        if let Some((name, tail)) = keyword_match(rest, &keywords) {
            let (head, _) = line.split_at(ws);
            let mut out = String::new();
            out.push_str(head);
            out.push_str("\x1b[35;1m");
            out.push_str(name);
            out.push_str("\x1b[0m");
            out.push_str(tail);
            return out;
        }
        let builtins: [&'static str; 10] = [
            "echo", "cd", "read", "local", "export", "source", "set", "trap", "shift", "exec",
        ];
        if let Some((name, tail)) = keyword_match(rest, &builtins) {
            let (head, _) = line.split_at(ws);
            let mut out = String::new();
            out.push_str(head);
            out.push_str("\x1b[34;1m");
            out.push_str(name);
            out.push_str("\x1b[0m");
            out.push_str(tail);
            return out;
        }
    }

    line.to_string()
}

fn make_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-')
}

fn run_while(s: &str, pred: impl Fn(char) -> bool) -> usize {
    s.chars().take_while(|c| pred(*c)).count()
}

fn keyword_match<'a>(
    rest: &'a str,
    words: &[&'static str],
) -> Option<(&'static str, &'a str)> {
    for w in words {
        if let Some(tail) = rest.strip_prefix(w) {
            let ok = tail.is_empty()
                || tail.starts_with(|c: char| is_ws(c) || c == ';');
            if ok {
                return Some((w, tail));
            }
        }
    }
    None
}