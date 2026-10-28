use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::fs;

#[derive(Clone)]
pub struct User {
    pub name: String,
    pub uid: u32,
    pub gid: u32,
    pub gecos: String,
    pub home: String,
    pub shell: String,
}

fn legacy_hash(password: &str, salt: u32) -> u32 {
    let mut h: u32 = 0x811C9DC5 ^ salt;
    for b in password.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    for _ in 0..3 {
        h ^= h >> 15;
        h = h.wrapping_mul(0x2c1b_3c6d);
        h ^= h >> 12;
        h = h.wrapping_mul(0x297a_2d39);
        h ^= h >> 15;
    }
    h
}

const SALT_ALPHABET: &[u8] = b"./0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

fn new_salt() -> String {
    let random = fs::read_prefix("/dev/urandom", 16).unwrap_or_default();
    (0..16)
        .map(|i| {
            let byte = random.get(i).copied().unwrap_or_else(|| (random_salt() >> (i % 4 * 8)) as u8);
            SALT_ALPHABET[byte as usize % SALT_ALPHABET.len()] as char
        })
        .collect()
}

pub fn hash_password(password: &str) -> Option<String> {
    crate::sys::crypt(password, &format!("$6${}$", new_salt()))
}

fn same_bytes(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

pub fn check_hash(password: &str, fields: &[&str]) -> bool {
    match fields.first() {
        Some(hash) if hash.starts_with('$') => crate::sys::crypt(password, hash).map(|h| same_bytes(h.as_bytes(), hash.as_bytes())).unwrap_or(false),
        Some(salt) => match (u32::from_str_radix(salt, 16), fields.get(1).map(|h| u32::from_str_radix(h, 16))) {
            (Ok(salt), Some(Ok(hash))) => legacy_hash(password, salt) == hash,
            _ => false,
        },
        None => false,
    }
}

pub fn random_salt() -> u32 {
    let mut bytes = [0u8; 4];
    if let Some(data) = fs::read_prefix("/dev/urandom", 4) {
        for (i, b) in data.iter().take(4).enumerate() {
            bytes[i] = *b;
        }
    }
    let t = crate::sys::uptime_ms() as u32;
    u32::from_le_bytes(bytes) ^ t.rotate_left(13) ^ 0x9e37_79b9
}

fn with_root(root: &str, path: &str) -> String {
    let root = root.trim_end_matches('/');
    format!("{}{}", root, path)
}

pub fn parse_passwd(text: &str) -> Vec<User> {
    text.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split(':').collect();
            if f.len() < 7 {
                return None;
            }
            Some(User {
                name: String::from(f[0]),
                uid: f[2].parse().ok()?,
                gid: f[3].parse().ok()?,
                gecos: String::from(f[4]),
                home: String::from(f[5]),
                shell: String::from(f[6]),
            })
        })
        .collect()
}

pub fn users(root: &str) -> Vec<User> {
    fs::read_to_string(&with_root(root, "/etc/passwd")).map(|t| parse_passwd(&t)).unwrap_or_default()
}

pub fn find(root: &str, name: &str) -> Option<User> {
    users(root).into_iter().find(|u| u.name == name)
}

pub fn find_uid(root: &str, uid: u32) -> Option<User> {
    users(root).into_iter().find(|u| u.uid == uid)
}

pub fn name_of(uid: u32) -> String {
    find_uid("", uid).map(|u| u.name).unwrap_or_else(|| format!("{}", uid))
}

pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 32
        && name.chars().next().map(|c| c.is_ascii_lowercase() || c == '_').unwrap_or(false)
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

pub fn set_password(root: &str, name: &str, password: &str) -> Result<(), &'static str> {
    let path = with_root(root, "/etc/shadow");
    let text = fs::read_to_string(&path).unwrap_or_default();
    let hash = hash_password(password).ok_or("crypt(3) failed")?;
    let days = crate::sys::realtime().sec.max(0) / 86400;
    let entry = format!("{}:{}:{}:0:99999:7:::", name, hash, days);
    let mut out = String::new();
    let mut replaced = false;
    for line in text.lines() {
        if line.split(':').next() == Some(name) {
            out.push_str(&entry);
            out.push('\n');
            replaced = true;
        } else if !line.trim().is_empty() {
            out.push_str(line);
            out.push('\n');
        }
    }
    if !replaced {
        out.push_str(&entry);
        out.push('\n');
    }
    if !fs::write(&path, out.as_bytes()) {
        return Err("cannot write /etc/shadow");
    }
    crate::sys::chmod(&path, 0o600);
    Ok(())
}

pub fn add_user(root: &str, name: &str, shell: &str, sudo: bool) -> Result<User, &'static str> {
    if !valid_name(name) {
        return Err("invalid user name (lowercase letters, digits, - and _)");
    }
    let mut list = users(root);
    if list.iter().any(|u| u.name == name) {
        return Err("user already exists");
    }
    let uid = list.iter().map(|u| u.uid).filter(|u| *u >= 1000 && *u < 60000).max().map(|u| u + 1).unwrap_or(1000);
    let user = User {
        name: String::from(name),
        uid,
        gid: uid,
        gecos: String::from(name),
        home: format!("/home/{}", name),
        shell: String::from(shell),
    };
    list.push(user.clone());
    write_passwd(root, &list)?;
    set_group(root, name, Some(uid))?;
    for g in DEFAULT_GROUPS {
        let _ = set_member(root, g, name, true);
    }
    let home = with_root(root, &user.home);
    let _ = crate::sys::mkdir(&with_root(root, "/home"));
    let _ = crate::sys::mkdir(&home);
    crate::sys::chown(&home, uid);
    crate::sys::chmod(&home, 0o755);
    if sudo {
        set_sudo(root, name, true)?;
    }
    Ok(user)
}

pub fn write_passwd(root: &str, list: &[User]) -> Result<(), &'static str> {
    let mut out = String::new();
    for u in list {
        out.push_str(&format!("{}:x:{}:{}:{}:{}:{}\n", u.name, u.uid, u.gid, u.gecos, u.home, u.shell));
    }
    if fs::write(&with_root(root, "/etc/passwd"), out.as_bytes()) { Ok(()) } else { Err("cannot write /etc/passwd") }
}

pub fn remove_user(root: &str, name: &str) -> Result<(), &'static str> {
    if name == "root" {
        return Err("refusing to remove root");
    }
    let mut list = users(root);
    let before = list.len();
    list.retain(|u| u.name != name);
    if list.len() == before {
        return Err("no such user");
    }
    write_passwd(root, &list)?;
    set_group(root, name, None)?;
    strip_member(root, name)?;
    let shadow = with_root(root, "/etc/shadow");
    let text = fs::read_to_string(&shadow).unwrap_or_default();
    let kept: Vec<&str> = text.lines().filter(|l| l.split(':').next() != Some(name)).collect();
    fs::write(&shadow, (kept.join("\n") + "\n").as_bytes());
    let _ = set_sudo(root, name, false);
    Ok(())
}

fn set_group(root: &str, name: &str, gid: Option<u32>) -> Result<(), &'static str> {
    let path = with_root(root, "/etc/group");
    let text = fs::read_to_string(&path).unwrap_or_default();
    let mut lines: Vec<String> = text.lines().filter(|l| !l.trim().is_empty() && l.split(':').next() != Some(name)).map(String::from).collect();
    if let Some(gid) = gid {
        lines.push(format!("{}:x:{}:", name, gid));
    }
    let mut out = lines.join("\n");
    out.push('\n');
    if fs::write(&path, out.as_bytes()) { Ok(()) } else { Err("cannot write /etc/group") }
}

pub const DEFAULT_GROUPS: &[&str] = &["audio", "video", "input", "inet"];

pub const ADMIN_GROUPS: &[&str] = &["power", "sysconf"];

fn edit_groups(root: &str, f: impl Fn(&str, &mut Vec<String>)) -> Result<(), &'static str> {
    let path = with_root(root, "/etc/group");
    let text = fs::read_to_string(&path).unwrap_or_default();
    let mut out = String::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() < 3 {
            out.push_str(line);
            out.push('\n');
            continue;
        }
        let mut members: Vec<String> = fields.get(3).map(|m| m.split(',').filter(|s| !s.is_empty()).map(String::from).collect()).unwrap_or_default();
        f(fields[0], &mut members);
        out.push_str(&format!("{}:{}:{}:{}\n", fields[0], fields[1], fields[2], members.join(",")));
    }
    if fs::write(&path, out.as_bytes()) { Ok(()) } else { Err("cannot write /etc/group") }
}

pub fn set_member(root: &str, group: &str, name: &str, member: bool) -> Result<(), &'static str> {
    edit_groups(root, |g, members| {
        if g != group {
            return;
        }
        members.retain(|m| m != name);
        if member {
            members.push(String::from(name));
        }
    })
}

fn strip_member(root: &str, name: &str) -> Result<(), &'static str> {
    edit_groups(root, |_, members| members.retain(|m| m != name))
}

pub fn member_of(root: &str, name: &str) -> Vec<String> {
    fs::read_to_string(&with_root(root, "/etc/group"))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split(':').collect();
            let members = f.get(3)?;
            members.split(',').any(|m| m == name).then(|| String::from(f[0]))
        })
        .collect()
}

pub fn set_sudo(root: &str, name: &str, allowed: bool) -> Result<(), &'static str> {
    for g in ADMIN_GROUPS {
        let _ = set_member(root, g, name, allowed);
    }
    let path = with_root(root, "/etc/sudoers");
    let text = fs::read_to_string(&path).unwrap_or_default();
    let mut lines: Vec<String> = text.lines().filter(|l| l.trim() != name && !l.trim().is_empty()).map(String::from).collect();
    if allowed {
        lines.push(String::from(name));
    }
    let mut out = lines.join("\n");
    out.push('\n');
    if !fs::write(&path, out.as_bytes()) {
        return Err("cannot write /etc/sudoers");
    }
    crate::sys::chmod(&path, 0o600);
    Ok(())
}

pub fn verify_password(name: &str, password: &str) -> bool {
    let Some(shadow) = fs::read_to_string("/etc/shadow") else {
        return false;
    };
    shadow.lines().any(|line| {
        let f: Vec<&str> = line.split(':').collect();
        f.len() >= 2 && f[0] == name && check_hash(password, &f[1..])
    })
}

pub fn is_sudoer(name: &str) -> bool {
    name == "root" || fs::read_to_string("/etc/sudoers").map(|t| t.lines().any(|l| l.trim() == name)).unwrap_or(false)
}

pub fn group_ids() -> Vec<(String, u32)> {
    crate::fs::read_to_string("/etc/group")
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split(':').collect();
            Some((String::from(*f.first()?), f.get(2)?.parse().ok()?))
        })
        .collect()
}

pub fn group_members(group: &str) -> Vec<String> {
    crate::fs::read_to_string("/etc/group")
        .unwrap_or_default()
        .lines()
        .find(|l| l.split(':').next() == Some(group))
        .and_then(|l| l.split(':').nth(3))
        .map(|m| m.split(',').filter(|s| !s.is_empty()).map(String::from).collect())
        .unwrap_or_default()
}
