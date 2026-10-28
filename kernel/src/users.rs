use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::fs;

#[derive(Clone)]
pub struct UserRecord {
    pub name: String,
    pub uid: u32,
    pub gid: u32,
    pub home: String,
    pub shell: String,
}

fn passwd() -> Vec<UserRecord> {
    let text = {
        let mut guard = fs::VFS.lock();
        guard.as_mut().and_then(|v| v.read(fs::root_id(), "/etc/passwd").ok())
    };
    let Some(bytes) = text else {
        return Vec::new();
    };
    String::from_utf8_lossy(&bytes)
        .lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split(':').collect();
            if f.len() < 7 {
                return None;
            }
            Some(UserRecord { name: f[0].to_string(), uid: f[2].parse().ok()?, gid: f[3].parse().ok()?, home: f[5].to_string(), shell: f[6].to_string() })
        })
        .collect()
}

pub fn find_by_uid(uid: u32) -> Option<UserRecord> {
    passwd().into_iter().find(|u| u.uid == uid)
}
