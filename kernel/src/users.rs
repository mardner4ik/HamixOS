use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use spin::Mutex;

use crate::fs;

pub const ROOT_UID: u32 = 0;

#[derive(Clone)]
pub struct UserRecord {
    pub name: String,
    pub uid: u32,
    pub gid: u32,
    pub home: String,
    pub shell: String,
}

struct UserDb {
    users: Vec<UserRecord>,
    shadow: Vec<(String, String)>,
    sudoers: Vec<String>,
}

static USERDB: Mutex<Option<UserDb>> = Mutex::new(None);

fn hash_with_salt(password: &str, salt: u32) -> u32 {
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

pub fn make_shadow_entry(password: &str, salt: u32) -> String {
    format!("{:08x}:{:08x}", salt, hash_with_salt(password, salt))
}

fn parse_db() -> UserDb {
    let mut db = UserDb { users: Vec::new(), shadow: Vec::new(), sudoers: Vec::new() };
    let root = fs::root_id();
    let guard = fs::VFS.lock();
    let vfs = match guard.as_ref() {
        Some(v) => v,
        None => return db,
    };

    if let Ok(bytes) = vfs.read(root, "/etc/passwd") {
        let text = String::from_utf8_lossy(&bytes);
        for line in text.lines() {
            let f: Vec<&str> = line.split(':').collect();
            if f.len() < 7 {
                continue;
            }
            if let (Ok(uid), Ok(gid)) = (f[2].parse::<u32>(), f[3].parse::<u32>()) {
                db.users.push(UserRecord {
                    name: f[0].to_string(),
                    uid,
                    gid,
                    home: f[5].to_string(),
                    shell: f[6].to_string(),
                });
            }
        }
    }

    if let Ok(bytes) = vfs.read(root, "/etc/shadow") {
        let text = String::from_utf8_lossy(&bytes);
        for line in text.lines() {
            if let Some((name, hash)) = line.split_once(':') {
                db.shadow.push((name.to_string(), hash.to_string()));
            }
        }
    }

    if let Ok(bytes) = vfs.read(root, "/etc/sudoers") {
        let text = String::from_utf8_lossy(&bytes);
        for line in text.lines() {
            let l = line.trim();
            if l.is_empty() || l.starts_with('#') {
                continue;
            }
            db.sudoers.push(l.to_string());
        }
    }

    db
}

pub fn init() {
    *USERDB.lock() = Some(parse_db());
}

pub fn reload() {
    *USERDB.lock() = Some(parse_db());
}

pub fn find_by_name(name: &str) -> Option<UserRecord> {
    USERDB.lock().as_ref()?.users.iter().find(|u| u.name == name).cloned()
}

pub fn verify_password(name: &str, password: &str) -> bool {
    let entry = {
        let db = USERDB.lock();
        match db.as_ref().and_then(|d| d.shadow.iter().find(|(n, _)| n == name)) {
            Some((_, e)) => e.clone(),
            None => return false,
        }
    };
    match entry.split_once(':') {
        Some((salt_hex, hash_hex)) => match u32::from_str_radix(salt_hex, 16) {
            Ok(salt) => format!("{:08x}", hash_with_salt(password, salt)) == hash_hex,
            Err(_) => false,
        },
        None => false,
    }
}

pub fn is_sudoer(name: &str) -> bool {
    if name == "root" {
        return true;
    }
    USERDB.lock().as_ref().map(|d| d.sudoers.iter().any(|s| s == name)).unwrap_or(false)
}

pub fn set_password(name: &str, password: &str) -> Result<(), &'static str> {
    let user = find_by_name(name).ok_or("no such user")?;
    let root = fs::root_id();
    let salt = user.uid.wrapping_mul(2_654_435_761).wrapping_add(0x9e37_79b9);
    let new_entry = make_shadow_entry(password, salt);

    let mut guard = fs::VFS.lock();
    let vfs = guard.as_mut().ok_or("filesystem not mounted")?;
    let bytes = vfs.read(root, "/etc/shadow").unwrap_or_default();
    let text = String::from_utf8_lossy(&bytes);

    let mut out = String::new();
    let mut replaced = false;
    for line in text.lines() {
        if let Some((n, _)) = line.split_once(':') {
            if n == name {
                out.push_str(&format!("{}:{}\n", name, new_entry));
                replaced = true;
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    if !replaced {
        out.push_str(&format!("{}:{}\n", name, new_entry));
    }

    vfs.write(root, "/etc/shadow", out.as_bytes(), false, ROOT_UID)
        .map_err(|_| "cannot write /etc/shadow")?;
    drop(guard);
    reload();
    Ok(())
}

pub fn add_user(name: &str, uid: u32, gid: u32, password: &str) -> Result<(), &'static str> {
    if find_by_name(name).is_some() {
        return Err("user already exists");
    }
    let root = fs::root_id();
    let home = format!("/home/{}", name);

    {
        let mut guard = fs::VFS.lock();
        let vfs = guard.as_mut().ok_or("filesystem not mounted")?;

        let mut passwd = vfs.read(root, "/etc/passwd").unwrap_or_default();
        let line = format!("{}:x:{}:{}:{}:{}:/bin/hsh\n", name, uid, gid, name, home);
        passwd.extend_from_slice(line.as_bytes());
        vfs.write(root, "/etc/passwd", &passwd, false, ROOT_UID)
            .map_err(|_| "cannot write /etc/passwd")?;

        let _ = vfs.mkdir_all(&home, uid);
    }

    reload();
    set_password(name, password)
}
