#![cfg_attr(target_os = "hamix", no_std)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub mode: u32,
}

#[cfg(target_os = "hamix")]
mod imp {
    use super::Entry;
    use alloc::string::String;
    use alloc::vec::Vec;
    use hxstd::{env, fs, sys};

    pub fn read(path: &str) -> Option<Vec<u8>> {
        fs::read(path)
    }

    pub fn write(path: &str, data: &[u8]) -> bool {
        fs::write(path, data)
    }

    pub fn entries(dir: &str) -> Option<Vec<Entry>> {
        sys::read_dir(dir).map(|list| list.into_iter().map(|e| Entry { name: e.name, is_dir: e.is_dir, size: e.size, mode: e.mode }).collect())
    }

    pub fn exists(path: &str) -> bool {
        fs::exists(path)
    }

    pub fn is_dir(path: &str) -> bool {
        sys::stat(path).map(|s| s.is_dir()).unwrap_or(false)
    }

    pub fn mkdir(path: &str) -> bool {
        sys::mkdir(path) >= 0
    }

    pub fn remove(path: &str) -> bool {
        sys::unlink(path) >= 0
    }

    pub fn rename(from: &str, to: &str) -> bool {
        sys::rename(from, to) >= 0
    }

    pub fn chdir(path: &str) -> bool {
        sys::chdir(path) >= 0
    }

    pub fn stdout(text: &str) {
        let _ = sys::write(1, text.as_bytes());
    }

    pub fn chmod(path: &str, mode: u32) -> bool {
        sys::chmod(path, mode) >= 0
    }

    pub fn kill(pid: i64) -> Result<(), String> {
        let r = sys::kill(pid);
        if r < 0 { Err(String::from(sys::error_name(r))) } else { Ok(()) }
    }

    pub fn spawn(program: &str, args: &[&str]) -> Result<i64, String> {
        let r = sys::spawn(program, args, sys::SPAWN_DETACH);
        if r < 0 { Err(String::from(sys::error_name(r))) } else { Ok(r) }
    }

    pub struct Child {
        pid: i64,
        out: Option<u64>,
    }

    pub fn spawn_piped(program: &str, args: &[&str], input: Option<&str>) -> Result<Child, String> {
        let (out_r, out_w) = sys::pipe().map_err(|e| String::from(sys::error_name(e)))?;
        let stdin = match input {
            Some(_) => Some(sys::pipe().map_err(|e| String::from(sys::error_name(e)))?),
            None => None,
        };
        let pid = sys::spawn_io(program, args, 0, stdin.map(|(r, _)| r), Some(out_w), Some(out_w));
        sys::close(out_w);
        if let Some((r, w)) = stdin {
            sys::close(r);
            if pid >= 0 {
                sys::write(w, input.unwrap_or("").as_bytes());
            }
            sys::close(w);
        }
        if pid < 0 {
            sys::close(out_r);
            return Err(String::from(sys::error_name(pid)));
        }
        sys::set_nonblocking(out_r);
        Ok(Child { pid, out: Some(out_r) })
    }

    impl Child {
        pub fn pid(&self) -> i64 {
            self.pid
        }

        pub fn read(&mut self) -> Vec<u8> {
            let mut out = Vec::new();
            let Some(fd) = self.out else {
                return out;
            };
            let mut buf = [0u8; 4096];
            loop {
                let n = sys::read(fd, &mut buf);
                if n <= 0 {
                    break;
                }
                out.extend_from_slice(&buf[..n as usize]);
            }
            out
        }

        pub fn try_wait(&mut self) -> Option<i32> {
            let code = sys::waitpid(self.pid, true);
            if code == sys::EAGAIN {
                return None;
            }
            Some(code as i32)
        }
    }

    impl Drop for Child {
        fn drop(&mut self) {
            if let Some(fd) = self.out.take() {
                sys::close(fd);
            }
        }
    }

    pub fn args() -> Vec<String> {
        env::args().to_vec()
    }

    pub fn var(name: &str) -> Option<String> {
        env::var(name).map(String::from)
    }

    pub fn uid() -> u32 {
        sys::getuid().max(0) as u32
    }

    pub fn now_ms() -> u64 {
        sys::uptime_ms()
    }

    pub fn epoch_seconds() -> i64 {
        sys::realtime().sec
    }
}

#[cfg(not(target_os = "hamix"))]
mod imp {
    use super::Entry;

    pub fn read(path: &str) -> Option<Vec<u8>> {
        std::fs::read(path).ok()
    }

    pub fn write(path: &str, data: &[u8]) -> bool {
        std::fs::write(path, data).is_ok()
    }

    pub fn entries(dir: &str) -> Option<Vec<Entry>> {
        use std::os::unix::fs::PermissionsExt;
        let list = std::fs::read_dir(dir).ok()?;
        Some(
            list.filter_map(|e| e.ok())
                .map(|e| {
                    let meta = e.metadata().ok();
                    Entry {
                        name: e.file_name().to_string_lossy().into_owned(),
                        is_dir: meta.as_ref().map(|m| m.is_dir()).unwrap_or(false),
                        size: meta.as_ref().map(|m| m.len()).unwrap_or(0),
                        mode: meta.as_ref().map(|m| m.permissions().mode() & 0o7777).unwrap_or(0),
                    }
                })
                .collect(),
        )
    }

    pub fn exists(path: &str) -> bool {
        std::path::Path::new(path).exists()
    }

    pub fn is_dir(path: &str) -> bool {
        std::path::Path::new(path).is_dir()
    }

    pub fn mkdir(path: &str) -> bool {
        std::fs::create_dir(path).is_ok()
    }

    pub fn remove(path: &str) -> bool {
        std::fs::remove_file(path).is_ok() || std::fs::remove_dir(path).is_ok()
    }

    pub fn rename(from: &str, to: &str) -> bool {
        std::fs::rename(from, to).is_ok()
    }

    pub fn chdir(path: &str) -> bool {
        std::env::set_current_dir(path).is_ok()
    }

    pub fn stdout(text: &str) {
        use std::io::Write;
        let mut out = std::io::stdout();
        let _ = out.write_all(text.as_bytes());
        let _ = out.flush();
    }

    pub fn chmod(path: &str, mode: u32) -> bool {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).is_ok()
    }

    pub fn kill(pid: i64) -> Result<(), String> {
        match std::process::Command::new("kill").arg("-9").arg(pid.to_string()).status() {
            Ok(s) if s.success() => Ok(()),
            _ => Err(String::from("operation not permitted")),
        }
    }

    pub fn spawn(program: &str, args: &[&str]) -> Result<i64, String> {
        std::process::Command::new(program).args(args).spawn().map(|c| c.id() as i64).map_err(|e| e.to_string())
    }

    pub struct Child {
        child: std::process::Child,
        output: std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
        status: Option<i32>,
    }

    pub fn spawn_piped(program: &str, args: &[&str], input: Option<&str>) -> Result<Child, String> {
        use std::io::{Read, Write};
        use std::process::{Command, Stdio};
        let mut child = Command::new(program)
            .args(args)
            .stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| e.to_string())?;
        if let (Some(mut stdin), Some(text)) = (child.stdin.take(), input) {
            let _ = stdin.write_all(text.as_bytes());
        }
        let output = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let readers: Vec<Box<dyn Read + Send>> = [child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>), child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>)].into_iter().flatten().collect();
        for mut reader in readers {
            let sink = output.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                while let Ok(n) = reader.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    sink.lock().unwrap().extend_from_slice(&buf[..n]);
                }
            });
        }
        Ok(Child { child, output, status: None })
    }

    impl Child {
        pub fn pid(&self) -> i64 {
            self.child.id() as i64
        }

        pub fn read(&mut self) -> Vec<u8> {
            std::mem::take(&mut *self.output.lock().unwrap())
        }

        pub fn try_wait(&mut self) -> Option<i32> {
            if self.status.is_none() {
                self.status = self.child.try_wait().ok().flatten().map(|s| s.code().unwrap_or(1));
            }
            self.status
        }
    }

    pub fn args() -> Vec<String> {
        std::env::args().collect()
    }

    pub fn var(name: &str) -> Option<String> {
        std::env::var(name).ok()
    }

    pub fn uid() -> u32 {
        read_to_string("/proc/self/status")
            .and_then(|s| s.lines().find_map(|l| l.strip_prefix("Uid:")).and_then(|v| v.split_whitespace().next()).and_then(|v| v.parse().ok()))
            .unwrap_or(0)
    }

    fn read_to_string(path: &str) -> Option<String> {
        std::fs::read_to_string(path).ok()
    }

    pub fn now_ms() -> u64 {
        use std::time::Instant;
        static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
        START.get_or_init(Instant::now).elapsed().as_millis() as u64
    }

    pub fn epoch_seconds() -> i64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
    }
}

pub use imp::*;

mod meta;

pub use meta::*;

pub fn spawn_elevated(password: &str, program: &str, args: &[&str]) -> Result<Child, String> {
    if uid() == 0 {
        return spawn_piped(program, args, None);
    }
    let mut argv: Vec<&str> = alloc::vec!["-S", "--", program];
    argv.extend_from_slice(args);
    let input = alloc::format!("{}\n", password);
    spawn_piped("/usr/bin/sudo", &argv, Some(&input))
}

pub fn read_to_string(path: &str) -> Option<String> {
    read(path).map(|b| String::from_utf8_lossy(&b).into_owned())
}

pub fn home() -> String {
    var("HOME").filter(|h| !h.is_empty()).unwrap_or_else(|| String::from("/"))
}

pub fn user_names() -> Vec<(u32, String)> {
    read_to_string("/etc/passwd")
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split(':').collect();
            Some((f.get(2)?.parse().ok()?, String::from(*f.first()?)))
        })
        .collect()
}

pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < 4 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        alloc::format!("{} B", bytes)
    } else if value >= 100.0 {
        alloc::format!("{} {}", value as u64, UNITS[unit])
    } else {
        let tenths = (value * 10.0 + 0.5) as u64;
        alloc::format!("{}.{} {}", tenths / 10, tenths % 10, UNITS[unit])
    }
}
