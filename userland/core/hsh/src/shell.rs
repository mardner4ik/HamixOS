use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use hamix_std::{fs, sys};

use crate::lexer::{Part, Word};
use crate::parser::{self, Node, Redir, RedirKind};

#[derive(Clone, Copy)]
pub struct Io {
    pub input: u64,
    pub out: u64,
    pub err: u64,
}

impl Io {
    pub const STD: Io = Io { input: 0, out: 1, err: 2 };
}

pub fn write(fd: u64, text: &str) {
    let bytes = text.as_bytes();
    let mut done = 0;
    while done < bytes.len() {
        let n = sys::write(fd, &bytes[done..]);
        if n <= 0 {
            break;
        }
        done += n as usize;
    }
}

#[macro_export]
macro_rules! out {
    ($io:expr, $($arg:tt)*) => { $crate::shell::write($io.out, &alloc::format!($($arg)*)) };
}

#[macro_export]
macro_rules! outln {
    ($io:expr) => { $crate::shell::write($io.out, "\n") };
    ($io:expr, $($arg:tt)*) => {{ let mut s = alloc::format!($($arg)*); s.push('\n'); $crate::shell::write($io.out, &s) }};
}

#[macro_export]
macro_rules! errln {
    ($io:expr, $($arg:tt)*) => {{ let mut s = alloc::format!($($arg)*); s.push('\n'); $crate::shell::write($io.err, &s) }};
}

pub enum Flow {
    Normal,
    Break,
    Continue,
    Return,
}

pub const DEFAULT_PATH: &str = "/usr/bin:/bin:/sbin:/usr/sbin:/opt/linux/bin:/opt/linux/usr/bin";

pub fn binary_kind(path: &str) -> Option<&'static str> {
    let head = fs::read_prefix(path, 64)?;
    if head.len() < 64 || !head.starts_with(b"\x7fELF") {
        return None;
    }
    let u16_at = |b: &[u8], o: usize| u16::from_le_bytes([b[o], b[o + 1]]);
    let u32_at = |b: &[u8], o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    let u64_at = |b: &[u8], o: usize| u64::from_le_bytes(b[o..o + 8].try_into().unwrap());
    let e_type = u16_at(&head, 16);
    let phoff = u64_at(&head, 32) as usize;
    let phnum = u16_at(&head, 56) as usize;
    let phdrs_end = phoff.checked_add(phnum.checked_mul(56)?)?;
    if phdrs_end > 64 * 1024 {
        return None;
    }
    let phdrs = fs::read_prefix(path, phdrs_end)?;
    if phdrs.len() < phdrs_end {
        return None;
    }
    let mut interp = false;
    let mut notes = Vec::new();
    let mut lowest = u64::MAX;
    for i in 0..phnum {
        let ph = &phdrs[phoff + i * 56..phoff + (i + 1) * 56];
        match u32_at(ph, 0) {
            1 if u64_at(ph, 40) != 0 => lowest = lowest.min(u64_at(ph, 16)),
            3 => interp = true,
            4 => notes.push((u64_at(ph, 8) as usize, u64_at(ph, 32) as usize)),
            _ => {}
        }
    }
    let native_note = notes.iter().any(|&(offset, size)| {
        let end = offset.saturating_add(size.min(4096));
        fs::read_prefix(path, end)
            .map(|data| {
                let note = &data[offset.min(data.len())..];
                note.len() >= 20 && u32_at(note, 0) == 6 && u32_at(note, 8) == 0x4858_4F53 && &note[12..18] == b"Hamix\0"
            })
            .unwrap_or(false)
    });
    Some(match (native_note, e_type, interp) {
        (true, _, _) => "native",
        (_, _, true) => "linux, dynamically linked",
        (_, 3, false) => "linux, static-pie",
        _ if lowest >= 0x80_0000_0000 => "native",
        _ => "linux, static (unsupported: not PIE)",
    })
}

pub struct Shell {
    pub vars: BTreeMap<String, String>,
    pub exported: Vec<String>,
    pub scoped: Vec<String>,
    pub args: Vec<String>,
    pub status: i32,
    pub history: Vec<String>,
    pub aliases: BTreeMap<String, String>,
    pub functions: BTreeMap<String, Node>,
    pub interactive: bool,
    pub login: bool,
    pub exit: Option<i32>,
    pub errexit: bool,
    pub xtrace: bool,
    pub flow: Flow,
    pub last_background: i64,
    pub jobs: Vec<(i64, String)>,
    pub depth: u32,
}

impl Shell {
    pub fn new() -> Shell {
        let mut vars = BTreeMap::new();
        let uid = sys::geteuid() as u32;
        let user = hamix_std::users::find_uid("", uid);
        let mut exported: Vec<String> = Vec::new();
        for (name, value) in hamix_std::env::vars() {
            if name != "PWD" {
                vars.insert(name.clone(), value.clone());
            }
            if !exported.contains(name) {
                exported.push(name.clone());
            }
        }
        let path = match vars.get("PATH") {
            Some(current) => {
                let mut dirs: Vec<&str> = current.split(':').filter(|d| !d.is_empty()).collect();
                for dir in DEFAULT_PATH.split(':') {
                    if !dirs.contains(&dir) {
                        dirs.push(dir);
                    }
                }
                dirs.join(":")
            }
            None => String::from(DEFAULT_PATH),
        };
        vars.insert(String::from("PATH"), path);
        vars.insert(String::from("SHELL"), String::from("/usr/bin/hsh"));
        vars.insert(String::from("USER"), user.as_ref().map(|u| u.name.clone()).unwrap_or_else(|| uid.to_string()));
        vars.insert(String::from("HOME"), user.as_ref().map(|u| u.home.clone()).unwrap_or_else(|| String::from("/")));
        vars.insert(String::from("HOSTNAME"), hostname());
        vars.insert(String::from("PWD"), sys::getcwd());
        vars.entry(String::from("LOGNAME")).or_insert_with(|| user.as_ref().map(|u| u.name.clone()).unwrap_or_else(|| uid.to_string()));
        vars.entry(String::from("TERM")).or_insert_with(|| String::from("xterm"));
        for name in ["PATH", "HOME", "USER", "LOGNAME", "SHELL", "PWD", "TERM"] {
            if !exported.iter().any(|e| e == name) {
                exported.push(String::from(name));
            }
        }
        Shell {
            vars,
            exported,
            scoped: Vec::new(),
            args: alloc::vec![String::from("hsh")],
            status: 0,
            history: Vec::new(),
            aliases: BTreeMap::new(),
            functions: BTreeMap::new(),
            interactive: false,
            login: false,
            exit: None,
            errexit: false,
            xtrace: false,
            flow: Flow::Normal,
            last_background: 0,
            jobs: Vec::new(),
            depth: 0,
        }
    }

    pub fn var(&self, name: &str) -> String {
        match name {
            "?" => self.status.to_string(),
            "#" => self.args.len().saturating_sub(1).to_string(),
            "$" => sys::getpid().to_string(),
            "!" => self.last_background.to_string(),
            "@" | "*" => self.args.iter().skip(1).cloned().collect::<Vec<_>>().join(" "),
            "PWD" => sys::getcwd(),
            "RANDOM" => (hamix_std::users::random_salt() % 32768).to_string(),
            "UID" => sys::getuid().to_string(),
            "EUID" => sys::geteuid().to_string(),
            n if n.chars().all(|c| c.is_ascii_digit()) && !n.is_empty() => {
                let index: usize = n.parse().unwrap_or(usize::MAX);
                self.args.get(index).cloned().unwrap_or_default()
            }
            n => {
                if let Some((base, default)) = n.split_once(":-") {
                    let value = self.var(base);
                    return if value.is_empty() { String::from(default) } else { value };
                }
                if let Some(base) = n.strip_prefix('#') {
                    return self.var(base).chars().count().to_string();
                }
                self.vars.get(n).cloned().unwrap_or_default()
            }
        }
    }

    pub fn set_var(&mut self, name: &str, value: &str) {
        if name == "PWD" {
            return;
        }
        self.vars.insert(String::from(name), String::from(value));
    }

    pub fn environment(&self) -> Vec<String> {
        let mut names: Vec<&String> = self.exported.iter().chain(self.scoped.iter()).collect();
        names.sort();
        names.dedup();
        names
            .into_iter()
            .filter(|name| name.as_str() == "PWD" || self.vars.contains_key(name.as_str()))
            .map(|name| format!("{}={}", name, self.var(name)))
            .collect()
    }

    pub fn spawn_with_env(&self, program: &str, args: &[&str], flags: u64, io: Io) -> i64 {
        let env = self.environment();
        sys::spawn_io_env(program, args, Some(env.as_slice()), flags, Some(io.input), Some(io.out), Some(io.err))
    }

    pub fn home(&self) -> String {
        self.var("HOME")
    }

    fn capture(&mut self, src: &str) -> String {
        let Ok((read_fd, write_fd)) = sys::pipe() else {
            return String::new();
        };
        let env = self.environment();
        let pid = sys::spawn_io_env("/usr/bin/hsh", &["-c", src], Some(env.as_slice()), 0, None, Some(write_fd), None);
        sys::close(write_fd);
        let mut output = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            let n = sys::read(read_fd, &mut buf);
            if n <= 0 {
                break;
            }
            output.extend_from_slice(&buf[..n as usize]);
        }
        sys::close(read_fd);
        if pid > 0 {
            self.status = sys::waitpid(pid, false) as i32;
        }
        let mut text = String::from_utf8_lossy(&output).into_owned();
        while text.ends_with('\n') {
            text.pop();
        }
        text
    }

    pub fn expand_word(&mut self, word: &Word) -> Vec<String> {
        let mut fields: Vec<String> = alloc::vec![String::new()];
        let mut has_glob = false;
        let mut quoted_any = false;
        for part in &word.parts {
            match part {
                Part::Lit(text, quoted) => {
                    if *quoted {
                        quoted_any = true;
                    } else if text.contains('*') || text.contains('?') {
                        has_glob = true;
                    }
                    fields.last_mut().unwrap().push_str(text);
                }
                Part::Tilde(user) => {
                    let home = if user.is_empty() {
                        self.home()
                    } else {
                        hamix_std::users::find("", user).map(|u| u.home).unwrap_or_else(|| format!("~{}", user))
                    };
                    fields.last_mut().unwrap().push_str(&home);
                }
                Part::Var(name, quoted) | Part::Sub(name, quoted) => {
                    let value = if let Part::Sub(src, _) = part { self.capture(src) } else if name == "@" && *quoted {
                        let items: Vec<String> = self.args.iter().skip(1).cloned().collect();
                        if items.is_empty() {
                            if fields.len() == 1 && fields[0].is_empty() {
                                return Vec::new();
                            }
                            continue;
                        }
                        let last = fields.last_mut().unwrap();
                        last.push_str(&items[0]);
                        for item in &items[1..] {
                            fields.push(item.clone());
                        }
                        quoted_any = true;
                        continue;
                    } else {
                        self.var(name)
                    };
                    if *quoted {
                        quoted_any = true;
                        fields.last_mut().unwrap().push_str(&value);
                    } else {
                        let mut pieces = value.split(|c: char| c == ' ' || c == '\t' || c == '\n').filter(|s| !s.is_empty());
                        if let Some(first) = pieces.next() {
                            fields.last_mut().unwrap().push_str(first);
                            for piece in pieces {
                                fields.push(String::from(piece));
                            }
                        }
                    }
                }
            }
        }
        if fields.len() == 1 && fields[0].is_empty() && !quoted_any {
            return Vec::new();
        }
        if has_glob && fields.len() == 1 {
            let matches = glob(&fields[0]);
            if !matches.is_empty() {
                return matches;
            }
        }
        fields
    }

    pub fn expand_words(&mut self, words: &[Word]) -> Vec<String> {
        let mut out = Vec::new();
        for word in words {
            out.extend(self.expand_word(word));
        }
        out
    }

    pub fn expand_one(&mut self, word: &Word) -> String {
        self.expand_word(word).join(" ")
    }

    pub fn run_source(&mut self, src: &str, io: Io) -> i32 {
        match parser::parse(src) {
            Ok(node) => self.run(&node, io),
            Err(e) => {
                errln!(io, "hsh: {}", e);
                self.status = 2;
                2
            }
        }
    }

    pub fn run(&mut self, node: &Node, io: Io) -> i32 {
        if self.exit.is_some() {
            return self.status;
        }
        let status = match node {
            Node::List(items) => {
                let mut status = self.status;
                for (item, background) in items {
                    if self.exit.is_some() || !matches!(self.flow, Flow::Normal) {
                        break;
                    }
                    status = if *background { self.run_background(item, io) } else { self.run(item, io) };
                    self.status = status;
                    if self.errexit && status != 0 && !matches!(item, Node::And(..) | Node::Or(..)) {
                        self.exit = Some(status);
                        break;
                    }
                }
                status
            }
            Node::And(a, b) => {
                let first = self.run_guarded(a, io);
                if first == 0 && matches!(self.flow, Flow::Normal) { self.run(b, io) } else { first }
            }
            Node::Or(a, b) => {
                let first = self.run_guarded(a, io);
                if first != 0 && matches!(self.flow, Flow::Normal) { self.run(b, io) } else { first }
            }
            Node::Pipeline(stages, negate) => {
                let status = self.run_pipeline(stages, io);
                if *negate { (status == 0) as i32 } else { status }
            }
            Node::If { branches, otherwise, .. } => {
                let mut result = 0;
                let mut taken = false;
                for (cond, body) in branches {
                    if self.run_guarded(cond, io) == 0 {
                        result = self.run(body, io);
                        taken = true;
                        break;
                    }
                }
                if !taken {
                    if let Some(body) = otherwise {
                        result = self.run(body, io);
                    }
                }
                result
            }
            Node::Loop { cond, body, until, .. } => {
                let mut result = 0;
                let mut guard = 0u64;
                loop {
                    let c = self.run_guarded(cond, io);
                    if (c == 0) == *until || self.exit.is_some() {
                        break;
                    }
                    result = self.run(body, io);
                    match self.flow {
                        Flow::Break => {
                            self.flow = Flow::Normal;
                            break;
                        }
                        Flow::Continue => self.flow = Flow::Normal,
                        Flow::Return => break,
                        Flow::Normal => {}
                    }
                    guard += 1;
                    if guard % 64 == 0 {
                        sys::yield_now();
                    }
                }
                result
            }
            Node::For { var, items, body, .. } => {
                let values = self.expand_words(items);
                let mut result = 0;
                for value in values {
                    self.set_var(var, &value);
                    result = self.run(body, io);
                    match self.flow {
                        Flow::Break => {
                            self.flow = Flow::Normal;
                            break;
                        }
                        Flow::Continue => self.flow = Flow::Normal,
                        Flow::Return => break,
                        Flow::Normal => {}
                    }
                    if self.exit.is_some() {
                        break;
                    }
                }
                result
            }
            Node::Group(body, redirs, _) => match self.open_redirs(redirs, io) {
                Ok((inner, opened)) => {
                    let status = self.run(body, inner);
                    for fd in opened {
                        sys::close(fd);
                    }
                    status
                }
                Err(e) => {
                    errln!(io, "hsh: {}", e);
                    1
                }
            },
            Node::Function(name, body) => {
                self.functions.insert(name.clone(), (**body).clone());
                0
            }
            Node::Simple { .. } => self.run_simple(node, io, false),
        };
        self.status = status;
        status
    }

    fn run_guarded(&mut self, node: &Node, io: Io) -> i32 {
        let saved = self.errexit;
        self.errexit = false;
        let status = self.run(node, io);
        self.errexit = saved;
        status
    }

    fn run_background(&mut self, node: &Node, io: Io) -> i32 {
        let src = node.source();
        let src = if src.is_empty() { String::from("true") } else { src };
        let pid = self.spawn_with_env("/usr/bin/hsh", &["-c", src.as_str()], sys::SPAWN_DETACH, io);
        if pid < 0 {
            errln!(io, "hsh: cannot start background job: {}", sys::error_name(pid));
            return 1;
        }
        self.last_background = pid;
        self.jobs.push((pid, src));
        if self.interactive {
            outln!(io, "[{}] {}", self.jobs.len(), pid);
        }
        0
    }

    fn open_redirs(&mut self, redirs: &[Redir], io: Io) -> Result<(Io, Vec<u64>), String> {
        let mut result = io;
        let mut opened = Vec::new();
        for redir in redirs {
            let target = redir.target.as_ref().map(|w| self.expand_one(w)).unwrap_or_default();
            let path = if target == "/dev/null" { String::from("/dev/null") } else { target.clone() };
            let open = |flags: u64| -> Result<u64, String> {
                let fd = sys::open_with(&path, flags);
                if fd < 0 { Err(format!("{}: {}", path, sys::error_name(fd))) } else { Ok(fd as u64) }
            };
            match redir.kind {
                RedirKind::In => {
                    let fd = open(sys::O_RDONLY)?;
                    opened.push(fd);
                    result.input = fd;
                }
                RedirKind::Out | RedirKind::AllOut => {
                    let fd = open(sys::O_WRONLY | sys::O_CREAT | sys::O_TRUNC)?;
                    opened.push(fd);
                    result.out = fd;
                    if redir.kind == RedirKind::AllOut {
                        result.err = fd;
                    }
                }
                RedirKind::Append => {
                    let fd = open(sys::O_WRONLY | sys::O_CREAT | sys::O_APPEND)?;
                    opened.push(fd);
                    result.out = fd;
                }
                RedirKind::ErrOut => {
                    let fd = open(sys::O_WRONLY | sys::O_CREAT | sys::O_TRUNC)?;
                    opened.push(fd);
                    result.err = fd;
                }
                RedirKind::ErrAppend => {
                    let fd = open(sys::O_WRONLY | sys::O_CREAT | sys::O_APPEND)?;
                    opened.push(fd);
                    result.err = fd;
                }
                RedirKind::ErrToOut => result.err = result.out,
            }
        }
        Ok((result, opened))
    }

    fn run_simple(&mut self, node: &Node, io: Io, in_pipeline: bool) -> i32 {
        let Node::Simple { assigns, words, redirs, .. } = node else {
            return self.run(node, io);
        };
        let mut argv = self.expand_words(words);
        if let Some(first) = argv.first().cloned() {
            if let Some(alias) = self.aliases.get(&first).cloned() {
                let mut expanded: Vec<String> = alias.split_whitespace().map(String::from).collect();
                expanded.extend(argv.drain(1..));
                argv = expanded;
            }
        }
        if argv.is_empty() {
            for (name, value) in assigns {
                let v = self.expand_one(value);
                self.set_var(name, &v);
            }
            return match self.open_redirs(redirs, io) {
                Ok((_, opened)) => {
                    for fd in opened {
                        sys::close(fd);
                    }
                    0
                }
                Err(e) => {
                    errln!(io, "hsh: {}", e);
                    1
                }
            };
        }
        let mut saved = Vec::new();
        let scoped_len = self.scoped.len();
        for (name, value) in assigns {
            let v = self.expand_one(value);
            saved.push((name.clone(), self.vars.get(name).cloned()));
            self.set_var(name, &v);
            self.scoped.push(name.clone());
        }
        if self.xtrace {
            errln!(io, "+ {}", argv.join(" "));
        }
        let (inner, opened) = match self.open_redirs(redirs, io) {
            Ok(v) => v,
            Err(e) => {
                errln!(io, "hsh: {}", e);
                return 1;
            }
        };
        let status = if let Some(body) = self.functions.get(&argv[0]).cloned() {
            let old_args = core::mem::replace(&mut self.args, argv.clone());
            let status = self.run(&body, inner);
            self.args = old_args;
            if matches!(self.flow, Flow::Return) {
                self.flow = Flow::Normal;
            }
            status
        } else if crate::builtins::is_builtin(&argv[0]) {
            crate::builtins::run(self, &argv, inner)
        } else {
            match self.spawn_external(&argv, inner, !in_pipeline) {
                Ok(pid) => self.wait_foreground(pid),
                Err(e) => {
                    errln!(io, "{}", e);
                    127
                }
            }
        };
        for fd in opened {
            sys::close(fd);
        }
        self.scoped.truncate(scoped_len);
        for (name, old) in saved {
            match old {
                Some(v) => {
                    self.vars.insert(name, v);
                }
                None => {
                    self.vars.remove(&name);
                }
            }
        }
        status
    }

    pub fn wait_foreground(&mut self, pid: i64) -> i32 {
        let code = sys::waitpid(pid, false);
        sys::set_foreground(0);
        let code = if code < 0 { 1 } else { code as i32 };
        if code == 130 && self.interactive {
            write(1, "\n");
        }
        code
    }

    pub fn resolve_program(&self, name: &str) -> Option<String> {
        if name.contains('/') {
            let path = if name.starts_with('/') { String::from(name) } else { format!("{}/{}", sys::getcwd().trim_end_matches('/'), name) };
            return sys::stat(&path).ok().filter(|s| !s.is_dir()).map(|_| path);
        }
        for dir in self.var("PATH").split(':').filter(|d| !d.is_empty()) {
            let candidate = format!("{}/{}", dir.trim_end_matches('/'), name);
            if let Ok(stat) = sys::stat(&candidate) {
                if !stat.is_dir() {
                    return Some(candidate);
                }
            }
        }
        None
    }

    pub fn spawn_external(&mut self, argv: &[String], io: Io, foreground: bool) -> Result<i64, String> {
        let name = &argv[0];
        let (program, mut args) = match self.resolve_program(name) {
            Some(path) => (path, argv[1..].to_vec()),
            None => match sys::cmd_list().into_iter().find(|c| &c.name == name) {
                Some(command) => {
                    let mut a = command.args.clone();
                    a.extend_from_slice(&argv[1..]);
                    (command.path, a)
                }
                None => return Err(format!("{}: command not found", name)),
            },
        };
        let head = fs::read_prefix(&program, 4).unwrap_or_default();
        let is_elf = head.starts_with(b"\x7fELF") || head.starts_with(b"\x7fHXL");
        let has_shebang = head.starts_with(b"#!");
        let executable = sys::stat(&program).map(|s| s.mode & 0o111 != 0).unwrap_or(false) || sys::geteuid() == 0;
        let program = if !is_elf && (!has_shebang || !executable) {
            args.insert(0, program);
            String::from("/usr/bin/hsh")
        } else {
            program
        };
        let flags = if foreground && self.interactive { sys::SPAWN_FOREGROUND } else { 0 };
        let args: Vec<&str> = args.iter().map(|a| a.as_str()).collect();
        let pid = self.spawn_with_env(&program, &args, flags, io);
        if pid < 0 {
            if pid == -8 {
                return Err(format!("{}: cannot execute binary ({})", name, binary_kind(&program).unwrap_or("unknown format")));
            }
            return Err(format!("{}: {}", name, sys::error_name(pid)));
        }
        Ok(pid)
    }

    fn run_pipeline(&mut self, stages: &[Node], io: Io) -> i32 {
        if stages.len() == 1 {
            return self.run(&stages[0], io);
        }
        let mut pids: Vec<i64> = Vec::new();
        let mut input = io.input;
        let mut to_close: Vec<u64> = Vec::new();
        for (index, stage) in stages.iter().enumerate() {
            let last = index + 1 == stages.len();
            let (next_read, out) = if last {
                (None, io.out)
            } else {
                match sys::pipe() {
                    Ok((r, w)) => (Some(r), w),
                    Err(e) => {
                        errln!(io, "hsh: pipe: {}", sys::error_name(e));
                        return 1;
                    }
                }
            };
            let stage_io = Io { input, out, err: io.err };
            let external = match stage {
                Node::Simple { words, assigns, redirs, .. } if assigns.is_empty() => {
                    let argv = self.expand_words(words);
                    if !argv.is_empty() && !crate::builtins::is_builtin(&argv[0]) && !self.functions.contains_key(&argv[0]) && !self.aliases.contains_key(&argv[0]) {
                        Some((argv, redirs.clone()))
                    } else {
                        None
                    }
                }
                _ => None,
            };
            let pid = match external {
                Some((argv, redirs)) => match self.open_redirs(&redirs, stage_io) {
                    Ok((inner, opened)) => {
                        let r = self.spawn_external(&argv, inner, false);
                        for fd in opened {
                            sys::close(fd);
                        }
                        r
                    }
                    Err(e) => Err(e),
                },
                None => {
                    let src = stage.source();
                    let pid = self.spawn_with_env("/usr/bin/hsh", &["-c", src.as_str()], 0, stage_io);
                    if pid < 0 { Err(format!("hsh: {}", sys::error_name(pid))) } else { Ok(pid) }
                }
            };
            match pid {
                Ok(pid) => pids.push(pid),
                Err(e) => errln!(io, "{}", e),
            }
            if input != io.input {
                to_close.push(input);
                sys::close(input);
            }
            if !last {
                sys::close(out);
            }
            if let Some(r) = next_read {
                input = r;
            }
        }
        let _ = to_close;
        if let Some(&last) = pids.last() {
            if self.interactive {
                sys::set_foreground(last);
            }
        }
        let mut status = 0;
        for (i, pid) in pids.iter().enumerate() {
            let code = sys::waitpid(*pid, false);
            if i + 1 == pids.len() {
                status = if code < 0 { 1 } else { code as i32 };
            }
        }
        sys::set_foreground(0);
        status
    }

    pub fn run_script(&mut self, path: &str, args: &[String], io: Io) -> i32 {
        let Some(text) = fs::read_to_string(path) else {
            errln!(io, "hsh: {}: cannot read the script", path);
            return 127;
        };
        let old = core::mem::replace(&mut self.args, {
            let mut a = alloc::vec![String::from(path)];
            a.extend_from_slice(args);
            a
        });
        let status = self.run_source(&text, io);
        self.args = old;
        status
    }
}

pub fn hostname() -> String {
    fs::read_to_string("/etc/hostname").map(|s| String::from(s.trim())).filter(|s| !s.is_empty()).unwrap_or_else(|| String::from("hamix"))
}

fn wildcard(pattern: &[char], text: &[char]) -> bool {
    match (pattern.first(), text.first()) {
        (None, None) => true,
        (Some('*'), _) => wildcard(&pattern[1..], text) || (!text.is_empty() && wildcard(pattern, &text[1..])),
        (Some('?'), Some(_)) => wildcard(&pattern[1..], &text[1..]),
        (Some(p), Some(t)) if p == t => wildcard(&pattern[1..], &text[1..]),
        _ => false,
    }
}

pub fn matches_pattern(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    wildcard(&p, &t)
}

pub fn glob(pattern: &str) -> Vec<String> {
    let (dir, file_pattern) = match pattern.rfind('/') {
        Some(i) => (&pattern[..i + 1], &pattern[i + 1..]),
        None => ("", pattern),
    };
    if dir.contains('*') || dir.contains('?') {
        return Vec::new();
    }
    let listing_dir = if dir.is_empty() { String::from(".") } else { String::from(dir) };
    let Some(entries) = sys::read_dir(&listing_dir) else {
        return Vec::new();
    };
    let mut out: Vec<String> = entries
        .into_iter()
        .filter(|e| (file_pattern.starts_with('.') || !e.name.starts_with('.')) && matches_pattern(file_pattern, &e.name))
        .map(|e| format!("{}{}", dir, e.name))
        .collect();
    out.sort();
    out
}
