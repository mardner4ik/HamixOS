use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cmp::Ordering;

pub const ARCH: &str = hamix_std::sys::ARCH;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RepoKind {
    Alpine,
    Hamix,
}

#[derive(Clone)]
pub struct Repo {
    pub kind: RepoKind,
    pub name: String,
    pub url: String,
}

impl Repo {
    pub fn index_url(&self) -> String {
        format!("{}/{}/APKINDEX.tar.gz", self.url.trim_end_matches('/'), ARCH)
    }

    pub fn package_url(&self, pkg: &Pkg) -> String {
        format!("{}/{}/{}-{}.apk", self.url.trim_end_matches('/'), ARCH, pkg.name, pkg.version)
    }
}

pub fn parse_repos(text: &str) -> (Vec<Repo>, Vec<String>) {
    let mut repos = Vec::new();
    let mut problems = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() != 3 {
            problems.push(format!("line {}: expected '<alpine|hamix> <name> <url>'", n + 1));
            continue;
        }
        let kind = match fields[0] {
            "alpine" => RepoKind::Alpine,
            "hamix" => RepoKind::Hamix,
            other => {
                problems.push(format!("line {}: unknown repository type '{}'", n + 1, other));
                continue;
            }
        };
        repos.push(Repo { kind, name: fields[1].to_string(), url: fields[2].to_string() });
    }
    (repos, problems)
}

#[derive(Clone, Default)]
pub struct Pkg {
    pub name: String,
    pub version: String,
    pub arch: String,
    pub size: u64,
    pub installed_size: u64,
    pub description: String,
    pub url: String,
    pub license: String,
    pub origin: String,
    pub checksum: String,
    pub depends: Vec<String>,
    pub provides: Vec<String>,
    pub install_if: Vec<String>,
    pub priority: i64,
    pub repo: usize,
}

pub fn parse_index(text: &str, repo: usize) -> Vec<Pkg> {
    let mut out = Vec::new();
    for block in text.split("\n\n") {
        let mut pkg = Pkg { repo, ..Default::default() };
        for line in block.lines() {
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let words = || value.split_whitespace().map(|s| s.to_string()).collect::<Vec<_>>();
            match key {
                "P" => pkg.name = value.to_string(),
                "V" => pkg.version = value.to_string(),
                "A" => pkg.arch = value.to_string(),
                "S" => pkg.size = value.parse().unwrap_or(0),
                "I" => pkg.installed_size = value.parse().unwrap_or(0),
                "T" => pkg.description = value.to_string(),
                "U" => pkg.url = value.to_string(),
                "L" => pkg.license = value.to_string(),
                "o" => pkg.origin = value.to_string(),
                "C" => pkg.checksum = value.to_string(),
                "D" => pkg.depends = words(),
                "p" => pkg.provides = words(),
                "i" => pkg.install_if = words(),
                "k" => pkg.priority = value.parse().unwrap_or(0),
                _ => {}
            }
        }
        if !pkg.name.is_empty() && !pkg.version.is_empty() && (pkg.arch.is_empty() || pkg.arch == ARCH || pkg.arch == "noarch") {
            out.push(pkg);
        }
    }
    out
}

pub struct Dep {
    pub name: String,
    pub op: Option<(String, String)>,
    pub conflict: bool,
}

pub fn parse_dep(token: &str) -> Dep {
    let (conflict, token) = match token.strip_prefix('!') {
        Some(rest) => (true, rest),
        None => (false, token),
    };
    let split = token.find(['<', '>', '=', '~']);
    match split {
        Some(i) => {
            let rest = &token[i..];
            let op_len = rest.find(|c: char| !matches!(c, '<' | '>' | '=' | '~')).unwrap_or(rest.len());
            Dep { name: token[..i].to_string(), op: Some((rest[..op_len].to_string(), rest[op_len..].to_string())), conflict }
        }
        None => Dep { name: token.to_string(), op: None, conflict },
    }
}

pub fn provided_name(token: &str) -> (&str, Option<&str>) {
    match token.split_once('=') {
        Some((name, version)) => (name, Some(version)),
        None => (token, None),
    }
}

impl Dep {
    pub fn satisfied_by(&self, version: &str) -> bool {
        let Some((op, wanted)) = &self.op else {
            return true;
        };
        let ord = compare_versions(version, wanted);
        match op.as_str() {
            "=" | "==" => ord == Ordering::Equal,
            ">=" => ord != Ordering::Less,
            "<=" => ord != Ordering::Greater,
            ">" => ord == Ordering::Greater,
            "<" => ord == Ordering::Less,
            "~" | "~=" => version.starts_with(wanted.as_str()),
            "><" | "<>" => ord != Ordering::Equal,
            _ => true,
        }
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
enum Class {
    PreSuffix,
    End,
    Letter,
    PostSuffix,
    Revision,
    Number,
}

fn suffix_rank(name: &str) -> (Class, i32) {
    match name {
        "alpha" => (Class::PreSuffix, 0),
        "beta" => (Class::PreSuffix, 1),
        "pre" => (Class::PreSuffix, 2),
        "rc" => (Class::PreSuffix, 3),
        "cvs" => (Class::PostSuffix, 0),
        "svn" => (Class::PostSuffix, 1),
        "git" => (Class::PostSuffix, 2),
        "hg" => (Class::PostSuffix, 3),
        "p" => (Class::PostSuffix, 4),
        _ => (Class::PostSuffix, 5),
    }
}

fn tokens(version: &str) -> Vec<(Class, i64, String)> {
    let (main, revision) = match version.rsplit_once("-r") {
        Some((m, r)) if r.chars().all(|c| c.is_ascii_digit()) && !r.is_empty() => (m, Some(r)),
        _ => (version, None),
    };
    let mut out = Vec::new();
    let bytes = main.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i];
        if c.is_ascii_digit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            let digits = &main[start..i];
            out.push((Class::Number, digits.parse().unwrap_or(i64::MAX), digits.to_string()));
        } else if c == b'_' {
            i += 1;
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                i += 1;
            }
            let (class, rank) = suffix_rank(&main[start..i]);
            out.push((class, rank as i64, String::new()));
        } else if c.is_ascii_alphabetic() {
            out.push((Class::Letter, c as i64, String::new()));
            i += 1;
        } else {
            i += 1;
        }
    }
    if let Some(r) = revision {
        out.push((Class::Revision, r.parse().unwrap_or(0), String::new()));
    }
    out
}

pub fn compare_versions(a: &str, b: &str) -> Ordering {
    let ta = tokens(a);
    let tb = tokens(b);
    let n = ta.len().max(tb.len());
    for i in 0..n {
        let x = ta.get(i).cloned().unwrap_or((Class::End, 0, String::new()));
        let y = tb.get(i).cloned().unwrap_or((Class::End, 0, String::new()));
        if x.0 != y.0 {
            return x.0.cmp(&y.0);
        }
        let ord = if x.0 == Class::Number && i > 0 && (x.2.starts_with('0') || y.2.starts_with('0')) && ta.get(i - 1).map(|t| t.0) == Some(Class::Number) {
            x.2.cmp(&y.2)
        } else {
            x.1.cmp(&y.1)
        };
        if ord != Ordering::Equal {
            return ord;
        }
    }
    Ordering::Equal
}
