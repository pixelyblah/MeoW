use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

mod syntax;

const SEP: &str = "--------------------------------------------------------";

static ORIG_TERMIOS: OnceLock<libc::termios> = OnceLock::new();

fn home() -> String {
    std::env::var("HOME").unwrap_or_default()
}

fn term_rows() -> usize {
    unsafe {
        let mut ws: libc::winsize = std::mem::zeroed();
        if libc::ioctl(0, libc::TIOCGWINSZ, &mut ws) == 0 && ws.ws_row > 0 {
            ws.ws_row as usize
        } else {
            24
        }
    }
}

fn emit(s: &str) {
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(s.as_bytes());
    let _ = out.flush();
}

fn raw_on() {
    let mut t: libc::termios = unsafe { std::mem::zeroed() };
    unsafe {
        libc::tcgetattr(0, &mut t);
        let _ = ORIG_TERMIOS.set(t);
        libc::cfmakeraw(&mut t);
        t.c_iflag |= libc::ICRNL;
        libc::tcsetattr(0, libc::TCSANOW, &t);
    }
}

fn restore_term() {
    if let Some(orig) = ORIG_TERMIOS.get() {
        unsafe {
            libc::tcsetattr(0, libc::TCSAFLUSH, orig);
        }
    }
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(b"\x1b[H\x1b[2J\x1b[?25h");
    let _ = out.flush();
}

fn cleanup_and_exit(code: i32) -> ! {
    restore_term();
    std::process::exit(code);
}

fn read_byte() -> Option<u8> {
    let mut b: u8 = 0;
    let n = unsafe { libc::read(0, &mut b as *mut u8 as *mut libc::c_void, 1) };
    if n == 1 {
        Some(b)
    } else {
        None
    }
}

#[derive(PartialEq, Clone, Copy)]
enum Key {
    Ch(char),
    Eof,
}

fn read_key() -> Key {
    let b = match read_byte() {
        Some(b) => b,
        None => return Key::Eof,
    };
    if b < 0x80 {
        return Key::Ch(b as char);
    }
    let len = match b {
        0xC2..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF4 => 4,
        _ => 1,
    };
    let mut bytes = vec![b];
    for _ in 1..len {
        bytes.push(read_byte().unwrap_or(0));
    }
    match String::from_utf8(bytes) {
        Ok(s) => Key::Ch(s.chars().next().unwrap()),
        Err(_) => Key::Ch(b as char),
    }
}

fn read_escape() -> [u8; 2] {
    let mut pfd = libc::pollfd {
        fd: 0,
        events: libc::POLLIN,
        revents: 0,
    };
    let mut buf = [0u8; 2];
    let r = unsafe { libc::poll(&mut pfd, 1, 10) };
    if r > 0 {
        unsafe {
            libc::read(0, buf.as_mut_ptr() as *mut libc::c_void, 2);
        }
    }
    buf
}

fn char_at(s: &str, i: usize) -> Option<char> {
    s.chars().nth(i)
}

fn char_slice(s: &str, start: usize, end: usize) -> String {
    s.chars()
        .skip(start)
        .take(if end == usize::MAX {
            usize::MAX
        } else {
            end.saturating_sub(start)
        })
        .collect()
}

fn db_path() -> PathBuf {
    std::env::var("MW_DB")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(format!("{}/.local/share/mw/db", home())))
}

fn db_mkfile(db: &Path) {
    if let Some(p) = db.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    if !db.exists() {
        let _ = std::fs::OpenOptions::new().create(true).write(true).open(db);
    }
}

fn db_read(db: &Path) -> Vec<(u64, String)> {
    let mut out = Vec::new();
    if let Ok(content) = std::fs::read_to_string(db) {
        for line in content.lines() {
            let line = line.trim_start();
            let mut it = line.splitn(2, ' ');
            let score = it.next().unwrap_or("");
            let entry = it.next().unwrap_or("").trim_end().to_string();
            if entry.is_empty() {
                continue;
            }
            out.push((score.parse::<u64>().unwrap_or(0), entry));
        }
    }
    out
}

fn db_write(db: &Path, entries: &[(u64, String)]) {
    let mut s = String::new();
    for (score, entry) in entries {
        s.push_str(&format!("{} {}\n", score, entry));
    }
    let _ = std::fs::write(db, s);
}

fn db_add_path(db: &Path, path: &str) {
    if path.is_empty() {
        return;
    }
    db_mkfile(db);
    let mut out: Vec<(u64, String)> = Vec::new();
    let mut found = false;
    for (score, entry) in db_read(db) {
        if entry == path {
            out.push((score.saturating_mul(9) / 10 + 10, entry));
            found = true;
        } else {
            let mut aged = score.saturating_mul(19) / 20;
            if aged < 1 {
                aged = 1;
            }
            out.push((aged, entry));
        }
    }
    if !found {
        out.push((10, path.to_string()));
    }
    out.sort_by(|a, b| b.0.cmp(&a.0));
    db_write(db, &out);
}

fn db_set_score(db: &Path, path: &str, score: u64) {
    if path.is_empty() {
        return;
    }
    db_mkfile(db);
    let mut out: Vec<(u64, String)> = Vec::new();
    let mut found = false;
    for (s, entry) in db_read(db) {
        if entry == path {
            out.push((score, entry));
            found = true;
        } else {
            out.push((s, entry));
        }
    }
    if !found {
        out.push((score, path.to_string()));
    }
    out.sort_by(|a, b| b.0.cmp(&a.0));
    db_write(db, &out);
}

fn db_remove_path(db: &Path, path: &str) {
    if path.is_empty() {
        return;
    }
    db_mkfile(db);
    let out: Vec<(u64, String)> = db_read(db)
        .into_iter()
        .filter(|(_, e)| e != path)
        .collect();
    db_write(db, &out);
}

fn db_score_match(query: &str, cand: &str) -> u64 {
    let bl = cand.rsplit('/').next().unwrap_or("").to_lowercase();
    let cl = cand.to_lowercase();
    let ql = query.to_lowercase();
    if bl.contains(&ql) {
        if bl == ql {
            return 1000;
        }
        let pos = bl.find(&ql).unwrap_or(0);
        return 500u64.saturating_sub(pos as u64);
    }
    if cl.contains(&ql) {
        let pos = cl.find(&ql).unwrap_or(0);
        return 200u64.saturating_sub(pos as u64);
    }
    let qchars: Vec<char> = ql.chars().collect();
    let cchars: Vec<char> = cl.chars().collect();
    let mut pj = 0usize;
    for ch in qchars {
        let mut ok = false;
        while pj < cchars.len() {
            if cchars[pj] == ch {
                pj += 1;
                ok = true;
                break;
            }
            pj += 1;
        }
        if !ok {
            return 0;
        }
    }
    50
}

#[allow(dead_code)]
fn db_query_path(db: &Path, query: &str) -> Option<String> {
    if query.is_empty() {
        return None;
    }
    db_mkfile(db);
    let mut best = String::new();
    let mut best_match: u64 = 0;
    for (score, entry) in db_read(db) {
        let m = db_score_match(query, &entry);
        if m > 0 {
            let total = m.saturating_mul(2) + score;
            if total > best_match {
                best_match = total;
                best = entry;
            }
        }
    }
    if best.is_empty() {
        None
    } else {
        Some(best)
    }
}

fn absolutize(path: &str) -> String {
    if let Ok(p) = std::fs::canonicalize(path) {
        return p.to_string_lossy().into_owned();
    }
    let p = PathBuf::from(path);
    if p.is_absolute() {
        path.to_string()
    } else if let Ok(cwd) = std::env::current_dir() {
        cwd.join(p).to_string_lossy().into_owned()
    } else {
        path.to_string()
    }
}

fn resolve_query(db: &Path, query: &str) -> Option<String> {
    if query.is_empty() {
        return None;
    }
    db_mkfile(db);
    let mut cands: Vec<(u64, String)> = db_read(db)
        .into_iter()
        .filter_map(|(score, entry)| {
            let m = db_score_match(query, &entry);
            if m > 0 {
                Some((m.saturating_mul(2) + score, entry))
            } else {
                None
            }
        })
        .collect();
    cands.sort_by(|a, b| b.0.cmp(&a.0));
    cands
        .into_iter()
        .map(|(_, e)| e)
        .find(|e| Path::new(e).exists())
}

fn db_list(db: &Path) {
    db_mkfile(db);
    for (score, entry) in db_read(db) {
        if Path::new(&entry).exists() {
            println!("{}\t{}", score, entry);
        } else {
            println!("{}\t{} (missing)", score, entry);
        }
    }
}

const TABSTOP: usize = 4;

fn expand_tabs_display(s: &str) -> String {
    let mut out = String::new();
    let mut col = 0usize;
    for c in s.chars() {
        if c == '\t' {
            let n = TABSTOP - (col % TABSTOP);
            for _ in 0..n {
                out.push(' ');
            }
            col += n;
        } else {
            out.push(c);
            col += 1;
        }
    }
    out
}

fn disp_col(line: &str, max_idx: usize) -> usize {
    let mut col = 0usize;
    for (i, c) in line.chars().enumerate() {
        if i >= max_idx {
            break;
        }
        if c == '\t' {
            col += TABSTOP - (col % TABSTOP);
        } else {
            col += 1;
        }
    }
    col
}

fn load_lines(path: &str) -> Vec<String> {
    let mut v = Vec::new();
    if let Ok(bytes) = std::fs::read(path) {
        let content = String::from_utf8_lossy(&bytes);
        if !content.is_empty() {
            v = content
                .split('\n')
                .map(|s| s.to_string())
                .collect::<Vec<String>>();
            if v.last().map(|s| s.is_empty()).unwrap_or(false) {
                v.pop();
            }
        }
    }
    v
}

struct Editor {
    file: String,
db: PathBuf,
    filetype: String,
    lines: Vec<String>,
    cursor_r: usize,
    cursor_c: usize,
    scroll_top: usize,
    mode: String,
    status_msg: String,
    term_rows: usize,
    view_height: usize,

    count: String,
    pending: String,
    search_pat: String,
    search_dir: i32,
    last_find_char: String,
    last_find_dir: i32,
    last_find_until: bool,
    marks: HashMap<char, (usize, usize)>,
    clipboard: String,
    clipboard_kind: String,
    undo_stack: Vec<Vec<String>>,
    redo_stack: Vec<Vec<String>>,
}

impl Editor {
    fn new(file: String, db: PathBuf, lines: Vec<String>) -> Editor {
        let filetype = syntax::detect_filetype(&file).to_string();
        let status_msg = format!("{} lines", lines.len());
        Editor {
            file,
db,
            filetype,
            lines,
            cursor_r: 0,
            cursor_c: 0,
            scroll_top: 0,
            mode: "NORMAL".to_string(),
            status_msg,
            term_rows: 24,
            view_height: 22,
            count: String::new(),
            pending: String::new(),
            search_pat: String::new(),
            search_dir: 1,
            last_find_char: String::new(),
            last_find_dir: 1,
            last_find_until: false,
            marks: HashMap::new(),
            clipboard: String::new(),
            clipboard_kind: "char".to_string(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    fn count_val(&self, dflt: usize) -> usize {
        if self.count.is_empty() {
            dflt
        } else {
            self.count.parse::<usize>().unwrap_or(dflt)
        }
    }

    fn cur_line(&self) -> String {
        self.lines.get(self.cursor_r).cloned().unwrap_or_default()
    }

    fn cur_len(&self) -> usize {
        self.lines
            .get(self.cursor_r)
            .map(|s| s.chars().count())
            .unwrap_or(0)
    }

    fn set_cur(&mut self, line: String) {
        if let Some(l) = self.lines.get_mut(self.cursor_r) {
            *l = line;
        }
    }

    fn clamp(&mut self) {
        let len = self.cur_len();
        if self.cursor_c > len {
            self.cursor_c = len;
        }
    }

    fn restore_clamp(&mut self) {
        let n = self.lines.len();
        if self.cursor_r >= n {
            self.cursor_r = n.saturating_sub(1);
        }
        let len = self.cur_len();
        if self.cursor_c > len {
            self.cursor_c = len;
        }
    }

    fn set_char(&mut self, idx: usize, ch: char) {
        let line = self.cur_line();
        let mut out: String = line.chars().take(idx).collect();
        out.push(ch);
        for rest in line.chars().skip(idx + 1) {
            out.push(rest);
        }
        self.set_cur(out);
    }

    fn redraw(&mut self) {
        self.term_rows = term_rows();
        self.view_height = self.term_rows.saturating_sub(2);

        if self.cursor_r < self.scroll_top {
            self.scroll_top = self.cursor_r;
        } else if self.cursor_r >= self.scroll_top + self.view_height {
            self.scroll_top = self.cursor_r + 1 - self.view_height;
        }

        let shape = if self.mode == "INSERT" {
            "\x1b[6 q"
        } else {
            "\x1b[2 q"
        };
        let mut buf = String::new();
        buf.push_str(shape);
        buf.push_str("\x1b[H");

        for i in 0..self.view_height {
            let line_idx = self.scroll_top + i;
            buf.push_str(&format!("\x1b[{};1H\x1b[K", i + 1));
            if line_idx < self.lines.len() {
                let shown = expand_tabs_display(&self.lines[line_idx]);
                buf.push_str(&syntax::highlight_line(&self.filetype, &shown));
            } else {
                buf.push_str("~");
            }
        }

        buf.push_str(&format!(
            "\x1b[{};1H{}\x1b[K",
            self.term_rows.saturating_sub(1),
            SEP
        ));
        buf.push_str(&format!(
            "\x1b[{};1H\x1b[7m -- {} -- | {} [{} {}] | {}\x1b[K\x1b[0m",
            self.term_rows, self.mode, self.file, self.filetype, self.file_size_hint(), self.status_msg
        ));

        let screen_r = self.cursor_r - self.scroll_top + 1;
        let screen_c = disp_col(&self.lines[self.cursor_r], self.cursor_c) + 1;
        buf.push_str(&format!("\x1b[{};{}H", screen_r, screen_c));

        emit(&buf);
    }

    fn run(&mut self) {
        loop {
            self.redraw();
            let key = read_key();

            if key == Key::Ch('\u{1b}') {
                self.handle_esc();
                continue;
            }

            if self.mode == "NORMAL" {
                self.normal_key(key);
            } else {
                self.insert_key(key);
            }
        }
    }

    fn handle_esc(&mut self) {
        let seq = read_escape();
        match &seq {
            b"[A" => {
                if self.cursor_r > 0 {
                    self.cursor_r -= 1
                }
            }
            b"[B" => {
                if self.cursor_r < self.lines.len() - 1 {
                    self.cursor_r += 1
                }
            }
            b"[C" => {
                let l = self.cur_len();
                if self.cursor_c < l {
                    self.cursor_c += 1
                }
            }
            b"[D" => {
                if self.cursor_c > 0 {
                    self.cursor_c -= 1
                }
            }
            _ => {
                self.mode = "NORMAL".to_string();
                self.pending.clear();
                self.count.clear();
            }
        }
        self.clamp();
    }

    fn file_size_hint(&self) -> String {
        let mut bytes: usize = 0;
        for (i, l) in self.lines.iter().enumerate() {
            if i > 0 {
                bytes += 1;
            }
            bytes += l.len();
        }
        bytes += 1;
        let mut out = String::new();
        let mut n = bytes;
        let mut steps = 0;
        while n >= 1024 && steps < 4 {
            n /= 1024;
            steps += 1;
        }
        let unit = ["B", "KiB", "MiB", "GiB"];
        out.push_str(&n.to_string());
        out.push(' ');
        out.push_str(unit[steps]);
        out
    }

    fn save_file(&mut self) {
        let mut content = String::new();
        for (i, l) in self.lines.iter().enumerate() {
            if i > 0 {
                content.push('\n');
            }
            content.push_str(l);
        }
        content.push('\n');
        let _ = std::fs::write(&self.file, content);
        db_add_path(&self.db, &absolutize(&self.file));
        self.status_msg = format!("\"{}\" written", self.file);
    }

    fn execute_command_mode(&mut self) {
        emit(&format!("\x1b[{};1H\x1b[K:", term_rows()));

        let mut cmd = String::new();
        loop {
            match read_key() {
                Key::Ch('\u{0a}') | Key::Eof => break,
                Key::Ch('\u{7f}') | Key::Ch('\u{08}') => {
                    if !cmd.is_empty() {
                        cmd.pop();
                        emit("\x08 \x08");
                    }
                }
                Key::Ch('\u{1b}') => {
                    cmd.clear();
                    break;
                }
                Key::Ch(ch) => {
                    cmd.push(ch);
                    emit(&ch.to_string());
                }
            }
        }

        match cmd.as_str() {
            "w" => self.save_file(),
            "q" | "q!" => cleanup_and_exit(0),
            "wq" | "x" | "wq!" => {
                self.save_file();
                cleanup_and_exit(0);
            }
            "" => self.status_msg.clear(),
            c if c.chars().next().map(|x| x.is_ascii_digit()).unwrap_or(false) => {
                let prefix: String = c
                    .split('.')
                    .next()
                    .unwrap_or(c)
                    .chars()
                    .take_while(|x| x.is_ascii_digit())
                    .collect();
                let num: usize = prefix.parse().unwrap_or(1);
                self.goto_line(num as i64 - 1);
                self.status_msg = format!("line {}", cmd);
            }
            _ => self.status_msg = format!("Unknown command: :{}", cmd),
        }
        self.mode = "NORMAL".to_string();
    }

    fn undo_save(&mut self) {
        self.undo_stack.push(self.lines.clone());
        self.redo_stack.clear();
    }

    fn undo(&mut self) {
        if self.undo_stack.is_empty() {
            self.status_msg = "Already at oldest change".to_string();
            return;
        }
        self.redo_stack.push(self.lines.clone());
        let last = self.undo_stack.pop().unwrap();
        self.lines = last;
        self.restore_clamp();
        self.status_msg = "Undone".to_string();
    }

    fn redo(&mut self) {
        if self.redo_stack.is_empty() {
            self.status_msg = "Already at newest change".to_string();
            return;
        }
        self.undo_stack.push(self.lines.clone());
        let last = self.redo_stack.pop().unwrap();
        self.lines = last;
        self.restore_clamp();
        self.status_msg = "Redone".to_string();
    }

    fn goto_line(&mut self, n: i64) {
        let n = n.clamp(0, self.lines.len() as i64 - 1) as usize;
        self.cursor_r = n;
        self.cursor_c = 0;
    }

    fn line_blank(s: &str) -> bool {
        s.chars().all(|c| c == ' ')
    }

    fn word_fwd(&mut self) {
        loop {
            let last = self.cursor_r == self.lines.len() - 1;
            let line = self.cur_line();
            let len = line.chars().count();
            let mut c = self.cursor_c;
            while c < len && char_at(&line, c) != Some(' ') {
                c += 1;
            }
            while c < len && char_at(&line, c) == Some(' ') {
                c += 1;
            }
            if c >= len {
                if last {
                    self.cursor_c = len;
                    return;
                }
                self.cursor_r += 1;
                self.cursor_c = 0;
            } else {
                self.cursor_c = c;
                return;
            }
        }
    }

    fn word_back(&mut self) {
        let mut c = self.cursor_c;
        let mut line = self.cur_line();
        while c > 0 && char_at(&line, c - 1) == Some(' ') {
            c -= 1;
        }
        while c > 0 && char_at(&line, c - 1) != Some(' ') {
            c -= 1;
        }
        if c == 0 && self.cursor_r > 0 {
            self.cursor_r -= 1;
            line = self.cur_line();
            c = line.chars().count();
            while c > 0 && char_at(&line, c - 1) == Some(' ') {
                c -= 1;
            }
            while c > 0 && char_at(&line, c - 1) != Some(' ') {
                c -= 1;
            }
        }
        self.cursor_c = c;
    }

    fn word_end(&mut self) {
        loop {
            let last = self.cursor_r == self.lines.len() - 1;
            let line = self.cur_line();
            let len = line.chars().count();
            let mut c = self.cursor_c;
            while c < len && char_at(&line, c) == Some(' ') {
                c += 1;
            }
            if c >= len {
                if !last {
                    self.cursor_r += 1;
                    self.cursor_c = 0;
                    continue;
                }
                return;
            }
            while c < len && char_at(&line, c) != Some(' ') {
                c += 1;
            }
            self.cursor_c = c - 1;
            return;
        }
    }

    fn first_nonblank(&mut self) {
        let line = self.cur_line();
        let len = line.chars().count();
        for (i, ch) in line.chars().enumerate() {
            if ch != ' ' {
                self.cursor_c = i;
                return;
            }
        }
        self.cursor_c = len;
    }

    fn line_at(&self, r: i64) -> &str {
        if r < 0 {
            &self.lines[self.lines.len() - 1]
        } else {
            &self.lines[r as usize]
        }
    }

    fn prev_para(&mut self) {
        let mut r = self.cursor_r as i64;
        loop {
            if r < 0 {
                break;
            }
            if !Editor::line_blank(self.line_at(r)) {
                r -= 1;
            }
            if Editor::line_blank(self.line_at(r)) {
                break;
            }
            r -= 1;
        }
        while r >= 0 {
            if Editor::line_blank(self.line_at(r)) {
                self.cursor_r = r as usize;
                self.cursor_c = 0;
                return;
            }
            r -= 1;
        }
        self.cursor_r = 0;
        self.cursor_c = 0;
    }

    fn next_para(&mut self) {
        let mut r = self.cursor_r;
        while r < self.lines.len() {
            if Editor::line_blank(&self.lines[r]) {
                self.cursor_r = r;
                self.cursor_c = 0;
                return;
            }
            r += 1;
        }
        self.cursor_r = self.lines.len() - 1;
        self.cursor_c = 0;
    }

    fn do_find(&mut self, ch: char, dir: i32, until: bool) {
        let line = self.cur_line();
        let len = line.chars().count();
        let mut found: Option<usize> = None;
        if dir == 1 {
            for i in (self.cursor_c + 1)..len {
                if char_at(&line, i) == Some(ch) {
                    found = Some(i);
                    break;
                }
            }
            if let Some(f) = found {
                self.cursor_c = if until { f.saturating_sub(1) } else { f };
            }
        } else {
            let mut i = self.cursor_c as i64 - 1;
            while i >= 0 {
                if char_at(&line, i as usize) == Some(ch) {
                    found = Some(i as usize);
                    break;
                }
                i -= 1;
            }
            if let Some(f) = found {
                self.cursor_c = if until { f + 1 } else { f };
            }
        }
        if found.is_some() {
            self.last_find_char = ch.to_string();
            self.last_find_dir = dir;
            self.last_find_until = until;
        }
        self.clamp();
    }

    fn repeat_find(&mut self, dir: i32) {
        if self.last_find_char.is_empty() {
            return;
        }
        let ch = self.last_find_char.chars().next().unwrap();
        let until = self.last_find_until;
        self.do_find(ch, dir, until);
    }

    fn search_prompt(&mut self, dir: i32, prompt_char: &str) {
        emit(&format!(
            "\x1b[{};1H\x1b[K{}",
            term_rows(),
            prompt_char
        ));
        let mut pat = String::new();
        loop {
            match read_key() {
                Key::Ch('\u{0a}') | Key::Eof => break,
                Key::Ch('\u{7f}') | Key::Ch('\u{08}') => {
                    if !pat.is_empty() {
                        pat.pop();
                        emit("\x08 \x08");
                    }
                }
                Key::Ch('\u{1b}') => {
                    pat.clear();
                    break;
                }
                Key::Ch(ch) => {
                    pat.push(ch);
                    emit(&ch.to_string());
                }
            }
        }
        if !pat.is_empty() {
            self.search_pat = pat;
            self.search_dir = dir;
            self.find_match(dir);
        }
        self.mode = "NORMAL".to_string();
    }

    fn find_match(&mut self, dir: i32) -> bool {
        let pat = self.search_pat.to_ascii_lowercase();
        if pat.is_empty() {
            self.status_msg = format!("Pattern not found: {}", self.search_pat);
            return false;
        }
        let n = self.lines.len();
        if dir == 1 {
            for i in self.cursor_r..n {
                let ll = self.lines[i].to_ascii_lowercase();
                let s = if i == self.cursor_r {
                    self.cursor_c + 1
                } else {
                    0
                };
                let llen = ll.chars().count();
                if s > llen {
                    continue;
                }
                let sub: String = ll.chars().skip(s).collect();
                if let Some(off) = sub.find(pat.as_str()) {
                    self.cursor_r = i;
                    self.cursor_c = s + off;
                    self.status_msg = format!("Search: {}", self.search_pat);
                    return true;
                }
            }
        } else {
            let mut i = self.cursor_r as i64;
            while i >= 0 {
                let ll = self.lines[i as usize].to_ascii_lowercase();
                let llen = ll.chars().count();
                let limit = if i as usize == self.cursor_r {
                    self.cursor_c
                } else {
                    llen
                };
                let mut found: Option<usize> = None;
                let mut s = 0usize;
                while s < limit {
                    let sub: String = ll.chars().skip(s).collect();
                    if let Some(off) = sub.find(pat.as_str()) {
                        if s + off < limit {
                            found = Some(s + off);
                            s = s + off + 1;
                        } else {
                            break;
                        }
                    } else {
                        break;
                    }
                }
                if let Some(f) = found {
                    self.cursor_r = i as usize;
                    self.cursor_c = f;
                    self.status_msg = format!("Search: {}", self.search_pat);
                    return true;
                }
                i -= 1;
            }
        }
        self.status_msg = format!("Pattern not found: {}", self.search_pat);
        false
    }

    fn word_under(&self) -> String {
        let line = self.cur_line();
        let c = self.cursor_c;
        let is_id = |ch: char| ch.is_ascii_alphanumeric() || ch == '_';
        if !char_at(&line, c).map(is_id).unwrap_or(false) {
            return String::new();
        }
        let mut s = c;
        let mut e = c;
        while s > 0 && char_at(&line, s - 1).map(is_id).unwrap_or(false) {
            s -= 1;
        }
        let l = line.chars().count();
        while e < l && char_at(&line, e).map(is_id).unwrap_or(false) {
            e += 1;
        }
        line.chars().skip(s).take(e - s).collect()
    }

    fn match_bracket(&mut self) {
        let line = self.cur_line();
        let len = line.chars().count();
        let is_b = |ch: char| matches!(ch, '(' | '[' | '{' | ')' | ']' | '}');
        let mut s = self.cursor_c;
        if !char_at(&line, s).map(is_b).unwrap_or(false) {
            s = usize::MAX;
            for i in self.cursor_c..len {
                if char_at(&line, i).map(is_b).unwrap_or(false) {
                    s = i;
                    break;
                }
            }
            if s == usize::MAX {
                return;
            }
        }
        let ch = char_at(&line, s).unwrap();
        match ch {
            ')' | ']' | '}' => {
                let open = match ch {
                    ')' => '(',
                    ']' => '[',
                    _ => '{',
                };
                let mut depth = 0usize;
                let mut i = s as i64 - 1;
                while i >= 0 {
                    let cc = char_at(&line, i as usize).unwrap();
                    if cc == open {
                        if depth == 0 {
                            self.cursor_c = i as usize;
                            return;
                        }
                        depth -= 1;
                    } else if cc == ch {
                        depth += 1;
                    }
                    i -= 1;
                }
            }
            '(' | '[' | '{' => {
                let close = match ch {
                    '(' => ')',
                    '[' => ']',
                    _ => '}',
                };
                let mut depth = 0usize;
                for i in (s + 1)..len {
                    let cc = char_at(&line, i).unwrap();
                    if cc == close {
                        if depth == 0 {
                            self.cursor_c = i;
                            return;
                        }
                        depth -= 1;
                    } else if cc == ch {
                        depth += 1;
                    }
                }
            }
            _ => {}
        }
    }

    fn togg_case(&mut self) {
        let line = self.cur_line();
        if line.chars().count() == 0 {
            return;
        }
        if let Some(ch) = char_at(&line, self.cursor_c) {
            if ch.is_ascii_lowercase() {
                self.set_char(self.cursor_c, ch.to_ascii_uppercase());
            } else if ch.is_ascii_uppercase() {
                self.set_char(self.cursor_c, ch.to_ascii_lowercase());
            }
        }
    }

    fn join_lines(&mut self) {
        if self.cursor_r >= self.lines.len() - 1 {
            return;
        }
        self.undo_save();
        let cur = self.cur_line();
        let nxt = self.lines[self.cursor_r + 1].clone();
        let joined = if cur.is_empty() {
            nxt
        } else {
            format!("{} {}", cur, nxt)
        };
        self.set_cur(joined);
        self.lines.remove(self.cursor_r + 1);
        self.cursor_c = self.cur_len();
    }

    fn clipboard_lines(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .clipboard
            .split('\n')
            .map(|s| s.to_string())
            .collect();
        if v.last().map(|s| s.is_empty()).unwrap_or(false) {
            v.pop();
        }
        v
    }

    fn paste_after(&mut self) {
        if self.clipboard.is_empty() {
            return;
        }
        self.undo_save();
        if self.clipboard_kind == "line" {
            let clines = self.clipboard_lines();
            let mut pbuf: Vec<String> = self.lines[..=self.cursor_r].to_vec();
            pbuf.extend(clines.clone());
            let rest = self.lines[self.cursor_r + 1..].to_vec();
            pbuf.extend(rest);
            self.lines = pbuf;
            self.cursor_r += clines.len();
            self.cursor_c = 0;
        } else {
            let l = self.cur_line();
            let c = self.cursor_c;
            let mut newl = char_slice(&l, 0, c);
            newl.push_str(&self.clipboard);
            newl.push_str(&char_slice(&l, c, usize::MAX));
            self.set_cur(newl);
            self.cursor_c = c + self.clipboard.chars().count();
        }
        self.clamp();
    }

    fn paste_before(&mut self) {
        if self.clipboard.is_empty() {
            return;
        }
        self.undo_save();
        if self.clipboard_kind == "line" {
            let clines = self.clipboard_lines();
            let mut pbuf: Vec<String> = self.lines[..self.cursor_r].to_vec();
            pbuf.extend(clines.clone());
            let rest = self.lines[self.cursor_r..].to_vec();
            pbuf.extend(rest);
            self.lines = pbuf;
            self.cursor_r = self.cursor_r + clines.len() - 1;
            self.cursor_c = 0;
        } else {
            let l = self.cur_line();
            let c = self.cursor_c;
            let mut newl = char_slice(&l, 0, c);
            newl.push_str(&self.clipboard);
            newl.push_str(&char_slice(&l, c, usize::MAX));
            self.set_cur(newl);
            self.cursor_c = c + self.clipboard.chars().count() - 1;
        }
        self.clamp();
    }

    fn delete_lines_range(&mut self, sr: usize, er: usize) {
        let n = self.lines.len();
        let er = er.min(n.saturating_sub(1));
        if sr > er {
            return;
        }
        self.undo_save();
        if er - sr + 1 >= n {
            self.lines = vec![String::new()];
            self.cursor_r = 0;
            self.cursor_c = 0;
            self.status_msg = "Buffer cleared".to_string();
            return;
        }
        let head = self.lines[..sr].to_vec();
        let tail = self.lines[er + 1..].to_vec();
        self.lines = [head, tail].concat();
        self.cursor_r = sr;
        if self.cursor_r >= self.lines.len() {
            self.cursor_r = self.lines.len() - 1;
        }
        self.cursor_c = 0;
    }

    fn apply_op(&mut self, op: char, mot: char) {
        let n = self.count_val(1).max(1);
        let sr = self.cursor_r;
        let sc = self.cursor_c;

        match mot {
            'w' | 'W' => {
                for _ in 0..n {
                    self.word_fwd();
                }
            }
            'b' | 'B' => {
                for _ in 0..n {
                    self.word_back();
                }
            }
            'e' | 'E' => {
                for _ in 0..n {
                    self.word_end();
                }
            }
            '$' => {
                let l = self.cur_len();
                self.cursor_c = l;
            }
            '^' => self.first_nonblank(),
            '0' => self.cursor_c = 0,
            'G' => self.goto_line(n as i64 - 1),
            'g' => self.goto_line(0),
            '}' => {
                self.next_para();
                self.cursor_c = 0;
            }
            '{' => {
                self.prev_para();
                self.cursor_c = 0;
            }
            'j' => {
                for _ in 0..n {
                    if self.cursor_r < self.lines.len() - 1 {
                        self.cursor_r += 1;
                    }
                }
            }
            'k' => {
                for _ in 0..n {
                    if self.cursor_r > 0 {
                        self.cursor_r -= 1;
                    }
                }
            }
            'd' | 'y' | 'c' => {
                for _ in 0..n.saturating_sub(1) {
                    if self.cursor_r < self.lines.len() - 1 {
                        self.cursor_r += 1;
                    }
                }
            }
            _ => {}
        }
        let er = self.cursor_r;
        let ec = self.cursor_c;

        let line_motion = matches!(mot, 'G' | 'g' | '}' | '{' | 'j' | 'k' | 'd' | 'y' | 'c');

        if line_motion {
            if op == 'c' && mot == 'c' {
                self.undo_save();
                self.lines[sr].clear();
                self.cursor_r = sr;
                self.cursor_c = 0;
                self.mode = "INSERT".to_string();
                return;
            }
            if op == 'd' {
                let nlines = self.lines.len();
                match mot {
                    'd' => self.delete_lines_range(sr, er),
                    'G' => self.delete_lines_range(sr, nlines.saturating_sub(1)),
                    'g' => self.delete_lines_range(0, er),
                    'j' => self.delete_lines_range(sr, er),
                    'k' => self.delete_lines_range(er, sr),
                    '}' => self.delete_lines_range(sr, er),
                    '{' => self.delete_lines_range(er, sr),
                    _ => {}
                }
            } else if op == 'c' {
                self.delete_lines_range(sr, er);
                self.mode = "INSERT".to_string();
            } else {
                let chunk = self.lines[sr..=er].to_vec();
                self.clipboard = chunk.join("\n");
                self.clipboard_kind = "line".to_string();
                self.cursor_r = sr;
                self.cursor_c = sc;
            }
            return;
        }

        if op == 'd' || op == 'c' {
            self.undo_save();
            let lo = sc.min(ec);
            let hi = sc.max(ec);
            if sr == er {
                let l = self.lines[sr].clone();
                self.lines[sr] =
                    format!("{}{}", char_slice(&l, 0, lo), char_slice(&l, hi, usize::MAX));
            } else {
                let l1 = self.lines[sr].clone();
                self.lines[sr] = char_slice(&l1, 0, sc);
                if er - sr > 1 {
                    let tail: Vec<String> = self.lines[er..].to_vec();
                    self.lines.truncate(sr + 1);
                    self.lines.extend(tail);
                }
                let l2 = self.lines[sr + 1].clone();
                self.lines[sr + 1] = char_slice(&l2, ec, usize::MAX);
            }
            self.cursor_r = sr;
            self.cursor_c = lo;
            self.clamp();
            if op == 'c' {
                self.mode = "INSERT".to_string();
            }
        } else if op == 'y' {
            let l = self.lines[sr].clone();
            let hi = if ec > sc { ec } else { sc };
            self.clipboard = char_slice(&l, sc, hi);
            self.clipboard_kind = "char".to_string();
            self.cursor_r = sr;
            self.cursor_c = sc;
        }
    }

    fn normal_key(&mut self, key: Key) {
        if !self.pending.is_empty() {
            match self.pending.as_str() {
                "d" | "c" | "y" => {
                    let Key::Ch(kk) = key else {
                        self.pending.clear();
                        return;
                    };
                    if kk.is_ascii_digit() {
                        self.count.push(kk);
                        return;
                    }
                    if matches!(
                        kk,
                        'w' | 'W' | 'b' | 'B' | 'e' | 'E' | '$' | '^' | '0' | 'G' | 'g' | '}'
                            | '{' | 'j' | 'k' | 'd' | 'y' | 'c'
                    ) {
                        let op = self.pending.chars().next().unwrap();
                        self.apply_op(op, kk);
                        self.pending.clear();
                        self.count.clear();
                    } else {
                        self.pending.clear();
                    }
                }
                "r" => {
                    match key {
                        Key::Ch(kk) => {
                            let rl = self.cur_line();
                            let l = rl.chars().count();
                            if l > 0 && self.cursor_c < l {
                                self.undo_save();
                                self.set_char(self.cursor_c, kk);
                            }
                        }
                        Key::Eof => {}
                    }
                    self.pending.clear();
                }
                "f" | "F" | "t" | "T" => {
                    if let Key::Ch(kk) = key {
                        match self.pending.as_str() {
                            "f" => self.do_find(kk, 1, false),
                            "F" => self.do_find(kk, -1, false),
                            "t" => self.do_find(kk, 1, true),
                            "T" => self.do_find(kk, -1, true),
                            _ => {}
                        }
                    }
                    self.pending.clear();
                    self.count.clear();
                }
                "g" => {
                    if key == Key::Ch('g') {
                        let n = self.count_val(1);
                        self.goto_line(n as i64 - 1);
                        self.pending.clear();
                        self.count.clear();
                    } else {
                        self.pending.clear();
                    }
                }
                "z" => {
                    match key {
                        Key::Ch('t') => self.scroll_top = self.cursor_r,
                        Key::Ch('z') => {
                            if self.scroll_top + (self.view_height / 2)
                                < self.lines.len()
                            {
                                self.scroll_top = self
                                    .cursor_r
                                    .saturating_sub(self.view_height / 2);
                            }
                        }
                        Key::Ch('b') => {
                            self.scroll_top = (self.cursor_r + 1)
                                .saturating_sub(self.view_height);
                        }
                        _ => {}
                    }
                    self.pending.clear();
                    self.count.clear();
                }
                "m" => {
                    if let Key::Ch(kk) = key {
                        if kk.is_ascii_alphabetic() {
                            self.marks
                                .insert(kk, (self.cursor_r, self.cursor_c));
                        }
                    }
                    self.pending.clear();
                }
                "mark_jump_exact" | "mark_jump_line" => {
                    if let Key::Ch(kk) = key {
                        if kk.is_ascii_alphabetic() {
                            if let Some(&(r, c)) = self.marks.get(&kk) {
                                self.cursor_r = r;
                                self.cursor_c = 0;
                                if self.pending == "mark_jump_exact" {
                                    self.cursor_c = c;
                                }
                            }
                        }
                    }
                    self.pending.clear();
                }
                "Z" => {
                    if key == Key::Ch('Z') {
                        self.save_file();
                        cleanup_and_exit(0);
                    }
                    self.pending.clear();
                }
                _ => self.pending.clear(),
            }
            return;
        }

        let Key::Ch(kk) = key else {
            if !self.count.is_empty() {
                self.status_msg = "Unknown command".to_string();
                self.count.clear();
            }
            return;
        };

        match kk {
            'i' => self.mode = "INSERT".to_string(),
            'a' => {
                self.mode = "INSERT".to_string();
                self.cursor_c += 1;
            }
            'I' => {
                self.mode = "INSERT".to_string();
                self.cursor_c = 0;
            }
            'A' => {
                self.mode = "INSERT".to_string();
                self.cursor_c = self.cur_len();
            }
            'o' => {
                self.undo_save();
                self.lines.insert(self.cursor_r + 1, String::new());
                self.cursor_r += 1;
                self.cursor_c = 0;
                self.mode = "INSERT".to_string();
            }
            'O' => {
                self.undo_save();
                self.lines.insert(self.cursor_r, String::new());
                self.cursor_c = 0;
                self.mode = "INSERT".to_string();
            }
            '0'..='9' => {
                if kk == '0' && self.count.is_empty() {
                    self.cursor_c = 0;
                } else {
                    self.count.push(kk);
                }
            }
            'h' => {
                let n = self.count_val(1);
                for _ in 0..n {
                    if self.cursor_c > 0 {
                        self.cursor_c -= 1;
                    }
                }
                self.count.clear();
            }
            'l' => {
                let n = self.count_val(1);
                for _ in 0..n {
                    let l = self.cur_len();
                    if self.cursor_c < l {
                        self.cursor_c += 1;
                    }
                }
                self.count.clear();
            }
            'j' => {
                let n = self.count_val(1);
                for _ in 0..n {
                    if self.cursor_r < self.lines.len() - 1 {
                        self.cursor_r += 1;
                    }
                }
                self.count.clear();
            }
            'k' => {
                let n = self.count_val(1);
                for _ in 0..n {
                    if self.cursor_r > 0 {
                        self.cursor_r -= 1;
                    }
                }
                self.count.clear();
            }
            'w' | 'W' => {
                let n = self.count_val(1);
                for _ in 0..n {
                    self.word_fwd();
                }
                self.count.clear();
            }
            'b' | 'B' => {
                let n = self.count_val(1);
                for _ in 0..n {
                    self.word_back();
                }
                self.count.clear();
            }
            'e' | 'E' => {
                let n = self.count_val(1);
                for _ in 0..n {
                    self.word_end();
                }
                self.count.clear();
            }
            '$' => {
                let l = self.cur_len();
                self.cursor_c = l;
            }
            '^' => self.first_nonblank(),
            'H' => {
                self.cursor_r = self.scroll_top;
                self.cursor_c = 0;
            }
            'M' => {
                self.cursor_r = self.scroll_top + (self.view_height / 2);
                if self.cursor_r >= self.lines.len() {
                    self.cursor_r = self.lines.len() - 1;
                }
                self.cursor_c = 0;
            }
            'L' => {
                self.cursor_r = self.scroll_top + self.view_height - 1;
                if self.cursor_r >= self.lines.len() {
                    self.cursor_r = self.lines.len() - 1;
                }
                self.cursor_c = 0;
            }
            'G' => {
                let n = self.count_val(0);
                if n > 0 {
                    self.goto_line(n as i64 - 1);
                } else {
                    self.goto_line(self.lines.len() as i64 - 1);
                }
                self.count.clear();
            }
            '{' => self.prev_para(),
            '}' => self.next_para(),
            '\u{04}' => {
                self.scroll_top += self.view_height / 2;
                self.cursor_r = self.scroll_top + (self.view_height / 2);
            }
            '\u{15}' => {
                let half = self.view_height / 2;
                self.scroll_top = self.scroll_top.saturating_sub(half);
                self.cursor_r = self.scroll_top;
            }
            '\u{06}' => {
                self.scroll_top += self.view_height;
                self.cursor_r = self.scroll_top + 1;
                if self.cursor_r >= self.lines.len() {
                    self.cursor_r = self.lines.len() - 1;
                }
            }
            '\u{02}' => {
                self.scroll_top = self.scroll_top.saturating_sub(self.view_height);
                self.cursor_r = self.scroll_top;
            }
            '%' => {
                self.match_bracket();
                self.pending.clear();
                self.count.clear();
            }
            'x' => {
                let n = self.count_val(1);
                if !self.cur_line().is_empty() {
                    let cr = self.cur_line();
                    let l = cr.chars().count();
                    if self.cursor_c < l {
                        self.undo_save();
                        let end = (self.cursor_c + n).min(l);
                        self.set_cur(format!(
                            "{}{}",
                            char_slice(&cr, 0, self.cursor_c),
                            char_slice(&cr, end, usize::MAX)
                        ));
                    }
                }
                self.count.clear();
            }
            'X' => {
                let n = self.count_val(1);
                if self.cursor_c > 0 {
                    let cr = self.cur_line();
                    self.undo_save();
                    let rn = n.min(self.cursor_c);
                    self.set_cur(format!(
                        "{}{}",
                        char_slice(&cr, 0, self.cursor_c - rn),
                        char_slice(&cr, self.cursor_c, usize::MAX)
                    ));
                    self.cursor_c -= rn;
                }
                self.count.clear();
            }
            's' => {
                self.undo_save();
                let cr = self.cur_line();
                let l = cr.chars().count();
                if l > 0 && self.cursor_c < l {
                    self.set_cur(format!(
                        "{}{}",
                        char_slice(&cr, 0, self.cursor_c),
                        char_slice(&cr, self.cursor_c + 1, usize::MAX)
                    ));
                }
                self.mode = "INSERT".to_string();
            }
            'S' => {
                self.undo_save();
                self.set_cur(String::new());
                self.cursor_c = 0;
                self.mode = "INSERT".to_string();
            }
            'D' => {
                self.undo_save();
                let cr = self.cur_line();
                self.set_cur(char_slice(&cr, 0, self.cursor_c));
            }
            'C' => {
                self.undo_save();
                let cr = self.cur_line();
                self.set_cur(char_slice(&cr, 0, self.cursor_c));
                self.mode = "INSERT".to_string();
            }
            'J' => self.join_lines(),
            '~' => {
                let n = self.count_val(1);
                for _ in 0..n {
                    let l = self.cur_len();
                    if self.cursor_c < l {
                        self.togg_case();
                        self.cursor_c += 1;
                    }
                }
                self.count.clear();
            }
            'r' => self.pending = "r".to_string(),
            'p' => {
                let n = self.count_val(1);
                for _ in 0..n {
                    self.paste_after();
                }
                self.count.clear();
            }
            'P' => {
                let n = self.count_val(1);
                for _ in 0..n {
                    self.paste_before();
                }
                self.count.clear();
            }
            'y' => self.pending = "y".to_string(),
            'd' => self.pending = "d".to_string(),
            'c' => self.pending = "c".to_string(),
            'f' => self.pending = "f".to_string(),
            'F' => self.pending = "F".to_string(),
            't' => self.pending = "t".to_string(),
            'T' => self.pending = "T".to_string(),
            ';' => self.repeat_find(1),
            ',' => self.repeat_find(-1),
            '/' => self.search_prompt(1, "/"),
            '?' => self.search_prompt(-1, "?"),
            'n' => {
                if !self.search_pat.is_empty() {
                    self.find_match(self.search_dir);
                }
            }
            'N' => {
                if !self.search_pat.is_empty() {
                    self.find_match(-self.search_dir);
                }
            }
            '*' => {
                let w = self.word_under();
                if !w.is_empty() {
                    self.search_pat = w;
                    self.search_dir = 1;
                    self.find_match(1);
                }
            }
            '#' => {
                let w = self.word_under();
                if !w.is_empty() {
                    self.search_pat = w;
                    self.search_dir = -1;
                    self.find_match(-1);
                }
            }
            'u' => self.undo(),
            '\u{12}' => self.redo(),
            'm' => self.pending = "m".to_string(),
            '`' => self.pending = "mark_jump_exact".to_string(),
            '\'' => self.pending = "mark_jump_line".to_string(),
            'g' => {
                self.pending = "g".to_string();
                self.count.clear();
            }
            'z' => {
                self.pending = "z".to_string();
                self.count.clear();
            }
            'Z' => self.pending = "Z".to_string(),
            ':' => self.execute_command_mode(),
            _ => {
                if !self.count.is_empty() {
                    self.status_msg = "Unknown command".to_string();
                }
                self.count.clear();
            }
        }

        self.clamp();
    }

    fn insert_key(&mut self, key: Key) {
        match key {
            Key::Ch('\u{7f}') | Key::Ch('\u{08}') => {
                let line = self.cur_line();
                let c = self.cursor_c;
                if c > 0 {
                    let mut newl: String = line.chars().take(c - 1).collect();
                    newl.extend(line.chars().skip(c));
                    self.set_cur(newl);
                    self.cursor_c -= 1;
                } else if self.cursor_r > 0 {
                    let prev = self.lines[self.cursor_r - 1].clone();
                    self.cursor_c = prev.chars().count();
                    let mut joined = prev;
                    joined.push_str(&line);
                    self.lines[self.cursor_r - 1] = joined;
                    self.lines.remove(self.cursor_r);
                    self.cursor_r -= 1;
                }
            }
            Key::Ch('\u{0a}') | Key::Eof => {
                let line = self.cur_line();
                let c = self.cursor_c;
                let left: String = line.chars().take(c).collect();
                let right: String = line.chars().skip(c).collect();
                self.set_cur(left);
                self.lines.insert(self.cursor_r + 1, right);
                self.cursor_r += 1;
                self.cursor_c = 0;
            }
            Key::Ch('\u{17}') => {
                let line = self.cur_line();
                let mut wc = self.cursor_c;
                while wc > 0 && char_at(&line, wc - 1) == Some(' ') {
                    wc -= 1;
                }
                while wc > 0 && char_at(&line, wc - 1) != Some(' ') {
                    wc -= 1;
                }
                if wc < self.cursor_c {
                    let mut newl = char_slice(&line, 0, wc);
                    newl.push_str(&char_slice(&line, self.cursor_c, usize::MAX));
                    self.set_cur(newl);
                    self.cursor_c = wc;
                }
            }
            Key::Ch('\u{15}') => {
                if self.cursor_c > 0 {
                    let line = self.cur_line();
                    self.set_cur(char_slice(&line, self.cursor_c, usize::MAX));
                    self.cursor_c = 0;
                }
            }
            Key::Ch(ch) => {
                let line = self.cur_line();
                let c = self.cursor_c;
                let mut newl: String = line.chars().take(c).collect();
                newl.push(ch);
                newl.extend(line.chars().skip(c));
                self.set_cur(newl);
                self.cursor_c += 1;
            }
        }
    }
}

fn main() {
    std::panic::set_hook(Box::new(|info| {
        restore_term();
        eprintln!("mw: {}", info);
    }));

    let args: Vec<String> = std::env::args().collect();
    let mut file = match args.get(1) {
        Some(f) if !f.is_empty() => f.clone(),
        _ => {
            eprintln!("Usage: mw <filename>");
            std::process::exit(1);
        }
    };

    let db = db_path();

    match file.as_str() {
        "-h" | "--help" => {
            print_help();
            std::process::exit(0);
        }
        "--list" => {
            db_list(&db);
            std::process::exit(0);
        }
        "--remove" => match args.get(2) {
            None => {
                eprintln!("Usage: mw --remove <path>");
                std::process::exit(1);
            }
            Some(p) => {
                db_remove_path(&db, &absolutize(p));
                std::process::exit(0);
            }
        },
        "--score" => match args.get(2) {
            None => {
                eprintln!("Usage: mw --score <path> [score]");
                std::process::exit(1);
            }
            Some(p) => {
                let n = args
                    .get(3)
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(100);
                db_set_score(&db, &absolutize(p), n);
                std::process::exit(0);
            }
        },
        "--db" => {
            println!("{}", db.display());
            std::process::exit(0);
        }
        _ => {}
    }

    raw_on();

    if !Path::new(&file).exists() {
        if let Some(res) = resolve_query(&db, &file) {
            file = res;
        }
    }
    file = absolutize(&file);
    db_add_path(&db, &file);

    let mut lines = load_lines(&file);
    if lines.is_empty() {
        lines.push(String::new());
    }

    let mut ed = Editor::new(file, db, lines);
    ed.run();
    cleanup_and_exit(0);
}

fn print_help() {
    print!(
        "\
Usage: mw <file>          open <file> (type just the name - file memory finds it)
       mw -h | --help     show this help
       mw --list          show remembered files by score
       mw --remove <p>    forget a file
       mw --score <p> [n] remember <p> at score n (default 100)
       mw --db            print the db path

:commands        :w save  :wq/:x/:wq! save+quit  :q/:q! quit  :N line
motions          h j k l / arrows  w b e  0 $ ^  G gg NG  {{ }}  %  H M L
insert           i a I A o O  s S C   then type, Esc to exit
edit             x X D dd dw de d$ dG  yy yw y$  p P  J ~  r<char>  u Ctrl-R
search           / ?  n N  * #  f<char> F<char> t<char> T<char> ; ,
marks            m<char> set   `<char> / '<char> jump
scroll           Ctrl-D / Ctrl-U half page   Ctrl-F / Ctrl-B page   zz center

counts work like vim: 3w, 5j, dd pasted with 2p
"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_db(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mw_db_test_{}_{}", tag, std::process::id()));
        let _ = std::fs::remove_file(&d);
        d
    }

    fn list(db: &Path) -> String {
        let mut s = String::new();
        for (score, entry) in db_read(db) {
            s.push_str(&format!("{}\t{}\n", score, entry));
        }
        s
    }

    #[test]
    fn db_matches_bash_reference() {
        let db = tmp_db("ref");
        db_remove_path(&db, "");

        db_add_path(&db, "/home/user/alpha.txt");
        db_add_path(&db, "/home/user/beta.txt");
        db_add_path(&db, "/home/user/alpha.txt");
        db_add_path(&db, "/home/user/gamma.txt");

        assert_eq!(
            list(&db),
            "17\t/home/user/alpha.txt\n10\t/home/user/gamma.txt\n8\t/home/user/beta.txt\n"
        );
        assert_eq!(
            db_query_path(&db, "beta"),
            Some("/home/user/beta.txt".to_string())
        );
        assert_eq!(
            db_query_path(&db, "/home/user/alpha.txt"),
            Some("/home/user/alpha.txt".to_string())
        );
        assert_eq!(
            db_query_path(&db, "alph"),
            Some("/home/user/alpha.txt".to_string())
        );
        assert_eq!(db_query_path(&db, "ab"), None);
        assert_eq!(
            db_query_path(&db, "gamm"),
            Some("/home/user/gamma.txt".to_string())
        );

        db_add_path(&db, "/home/user/gamma.txt");
        db_add_path(&db, "/home/user/gamma.txt");
        db_add_path(&db, "/home/user/gamma.txt");
        assert_eq!(
            db_query_path(&db, "gamma"),
            Some("/home/user/gamma.txt".to_string())
        );

        db_remove_path(&db, "/home/user/beta.txt");
        assert_eq!(
            list(&db),
            "34\t/home/user/gamma.txt\n14\t/home/user/alpha.txt\n"
        );
        assert_eq!(db_query_path(&db, "delt"), None);

        let _ = std::fs::remove_file(&db);
    }

    #[test]
    fn score_match_basics() {
        assert_eq!(db_score_match("beta", "/home/user/beta.txt"), 500);
        assert_eq!(db_score_match("/home/user/alpha.txt", "/home/user/alpha.txt"), 200);
        assert_eq!(db_score_match("ab", "/home/user/alpha.txt"), 0);
        assert_eq!(db_score_match("alp", "/home/user/alpha.txt"), 500);
        assert_eq!(db_score_match("axt", "/home/user/alpha.txt"), 50);
        assert_eq!(db_score_match("exact", "/x/exact"), 1000);
        assert_eq!(db_score_match("ExAcT", "/x/exact"), 1000);
    }

    #[test]
    fn set_score_pins_a_favorite() {
        let db = tmp_db("score");
        db_add_path(&db, "/home/user/other.txt");
        assert_eq!(list(&db), "10\t/home/user/other.txt\n");

        db_set_score(&db, "/home/user/fav.txt", 500);
        db_set_score(&db, "/home/user/other.txt", 3);
        assert_eq!(list(&db), "500\t/home/user/fav.txt\n3\t/home/user/other.txt\n");

        assert_eq!(
            db_query_path(&db, "fav"),
            Some("/home/user/fav.txt".to_string())
        );
        assert_eq!(
            db_query_path(&db, "oth"),
            Some("/home/user/other.txt".to_string())
        );

        db_set_score(&db, "/home/user/fresh.txt", 100);
        assert_eq!(
            db_query_path(&db, "fresh"),
            Some("/home/user/fresh.txt".to_string())
        );

        let _ = std::fs::remove_file(&db);
    }

    #[test]
    fn resolve_skips_ghost_entries() {
        let db = tmp_db("resolve");
        let suffix = format!("mw_{}", std::process::id());
        let real = std::env::temp_dir().join(format!("{}_real.txt", suffix));
        std::fs::write(&real, "x").unwrap();
        let ghost_p = format!("{}/{}_real.txt", std::env::temp_dir().display(), suffix);

        db_mkfile(&db);
        db_write(&db, &[(99, ghost_p.clone()), (10, real.to_string_lossy().into_owned())]);

        let q = format!("{}_", suffix);
        assert_eq!(db_query_path(&db, &q), Some(ghost_p));
        assert_eq!(resolve_query(&db, &q), Some(real.to_string_lossy().into_owned()));

        let _ = std::fs::remove_file(&real);
        let _ = std::fs::remove_file(&db);
    }

    #[test]
    fn absolutize_makes_relative_paths_absolute() {
        let abs = absolutize("src/main.rs");
        assert!(Path::new(&abs).is_absolute());
        let existing = absolutize("Cargo.toml");
        let real = std::fs::canonicalize("Cargo.toml").unwrap();
        assert_eq!(existing, real.to_string_lossy());
    }
}