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

pub fn hash_with_salt(password: &str, salt: u32) -> u32 {
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

pub fn shadow_entry(password: &str, salt: u32) -> String {
    format!("{:08x}:{:08x}", salt, hash_with_salt(password, salt))
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
    let entry = shadow_entry(password, random_salt());
    let mut out = String::new();
    let mut replaced = false;
    for line in text.lines() {
        if line.split(':').next() == Some(name) {
            out.push_str(&format!("{}:{}\n", name, entry));
            replaced = true;
        } else if !line.trim().is_empty() {
            out.push_str(line);
            out.push('\n');
        }
    }
    if !replaced {
        out.push_str(&format!("{}:{}\n", name, entry));
    }
    if !fs::write(&path, out.as_bytes()) {
        return Err("cannot write /etc/shadow");
    }
    crate::sys::chmod(&path, 0o600);
    if root.is_empty() {
        crate::sys::users_reload();
    }
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
    let home = with_root(root, &user.home);
    let _ = crate::sys::mkdir(&with_root(root, "/home"));
    let _ = crate::sys::mkdir(&home);
    crate::sys::chown(&home, uid);
    crate::sys::chmod(&home, 0o755);
    if sudo {
        set_sudo(root, name, true)?;
    }
    if root.is_empty() {
        crate::sys::users_reload();
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
    let shadow = with_root(root, "/etc/shadow");
    let text = fs::read_to_string(&shadow).unwrap_or_default();
    let kept: Vec<&str> = text.lines().filter(|l| l.split(':').next() != Some(name)).collect();
    fs::write(&shadow, (kept.join("\n") + "\n").as_bytes());
    let _ = set_sudo(root, name, false);
    if root.is_empty() {
        crate::sys::users_reload();
    }
    Ok(())
}

pub fn set_sudo(root: &str, name: &str, allowed: bool) -> Result<(), &'static str> {
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

pub fn is_sudoer(name: &str) -> bool {
    name == "root" || fs::read_to_string("/etc/sudoers").map(|t| t.lines().any(|l| l.trim() == name)).unwrap_or(false)
}
