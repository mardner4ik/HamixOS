pub mod tar;
pub mod ext4;

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use spin::Mutex;

pub const DEV_NULL: u8 = 0;
pub const DEV_ZERO: u8 = 1;
pub const DEV_CONSOLE: u8 = 2;
pub const DEV_RANDOM: u8 = 3;

pub const MODE_DIR_DEFAULT: u16 = 0o755;
pub const MODE_FILE_DEFAULT: u16 = 0o644;
pub const MODE_DEVICE_DEFAULT: u16 = 0o666;
pub const MODE_PROC_DEFAULT: u16 = 0o444;

pub enum NodeKind {
    Dir(BTreeMap<String, usize>),
    File(Vec<u8>),
    Device(u8),
    Proc(fn() -> String),
}

pub struct Node {
    kind: NodeKind,
    parent: usize,
    owner: u32,
    mode: u16,
}

pub struct Vfs {
    nodes: Vec<Node>,
}

impl Vfs {
    fn new() -> Self {
        let root = Node {
            kind: NodeKind::Dir(BTreeMap::new()),
            parent: 0,
            owner: 0,
            mode: MODE_DIR_DEFAULT,
        };
        Self { nodes: alloc::vec![root] }
    }

    pub fn root_id(&self) -> usize {
        0
    }

    fn push_node(&mut self, node: Node) -> usize {
        self.nodes.push(node);
        self.nodes.len() - 1
    }

    fn child(&self, dir: usize, name: &str) -> Option<usize> {
        match &self.nodes[dir].kind {
            NodeKind::Dir(m) => m.get(name).copied(),
            _ => None,
        }
    }

    pub fn resolve(&self, cwd: usize, path: &str) -> Option<usize> {
        let mut cur = if path.starts_with('/') { self.root_id() } else { cwd };
        for part in path.split('/') {
            if part.is_empty() || part == "." {
                continue;
            }
            if part == ".." {
                cur = self.nodes[cur].parent;
                continue;
            }
            cur = self.child(cur, part)?;
        }
        Some(cur)
    }

    fn resolve_parent<'a>(&self, cwd: usize, path: &'a str) -> Option<(usize, &'a str)> {
        let trimmed = path.trim_end_matches('/');
        if trimmed.is_empty() {
            return None;
        }
        let (dir_part, name) = match trimmed.rfind('/') {
            Some(idx) => (&trimmed[..idx], &trimmed[idx + 1..]),
            None => ("", trimmed),
        };
        if name.is_empty() || name == "." || name == ".." {
            return None;
        }
        let dir_id = if dir_part.is_empty() {
            if trimmed.starts_with('/') { self.root_id() } else { cwd }
        } else {
            self.resolve(cwd, dir_part)?
        };
        Some((dir_id, name))
    }

    pub fn owner_of(&self, id: usize) -> u32 {
        self.nodes[id].owner
    }

    pub fn mode_of(&self, id: usize) -> u16 {
        self.nodes[id].mode
    }

    pub fn can_write(&self, id: usize, uid: u32) -> bool {
        if uid == 0 {
            return true;
        }
        let node = &self.nodes[id];
        if node.owner == uid && node.mode & 0o200 != 0 {
            return true;
        }
        node.mode & 0o002 != 0
    }

    pub fn chmod(&mut self, cwd: usize, path: &str, uid: u32, mode: u16) -> Result<(), &'static str> {
        let id = self.resolve(cwd, path).ok_or("no such file or directory")?;
        if uid != 0 && self.nodes[id].owner != uid {
            return Err("permission denied");
        }
        self.nodes[id].mode = mode;
        Ok(())
    }

    pub fn chown(&mut self, cwd: usize, path: &str, uid: u32, new_owner: u32) -> Result<(), &'static str> {
        if uid != 0 {
            return Err("permission denied");
        }
        let id = self.resolve(cwd, path).ok_or("no such file or directory")?;
        self.nodes[id].owner = new_owner;
        Ok(())
    }

    pub fn mkdir(&mut self, cwd: usize, path: &str, owner: u32) -> Result<usize, &'static str> {
        let (parent, name) = self.resolve_parent(cwd, path).ok_or("bad path")?;
        if !self.can_write(parent, owner) {
            return Err("permission denied");
        }
        if self.child(parent, name).is_some() {
            return Err("already exists");
        }
        let id = self.push_node(Node {
            kind: NodeKind::Dir(BTreeMap::new()),
            parent,
            owner,
            mode: MODE_DIR_DEFAULT,
        });
        match &mut self.nodes[parent].kind {
            NodeKind::Dir(m) => { m.insert(name.to_string(), id); }
            _ => return Err("parent is not a directory"),
        }
        Ok(id)
    }

    pub fn mkdir_all(&mut self, path: &str, owner: u32) -> Result<usize, &'static str> {
        let root = self.root_id();
        let mut cur = root;
        let mut acc = String::new();
        for part in path.split('/') {
            if part.is_empty() {
                continue;
            }
            acc.push('/');
            acc.push_str(part);
            cur = match self.resolve(root, &acc) {
                Some(id) => id,
                None => self.mkdir(root, &acc, owner)?,
            };
        }
        Ok(cur)
    }

    pub fn create_file(&mut self, cwd: usize, path: &str, data: Vec<u8>, owner: u32) -> Result<usize, &'static str> {
        let (parent, name) = self.resolve_parent(cwd, path).ok_or("bad path")?;
        if !self.can_write(parent, owner) {
            return Err("permission denied");
        }
        if let Some(existing) = self.child(parent, name) {
            if !self.can_write(existing, owner) {
                return Err("permission denied");
            }
            if let NodeKind::File(buf) = &mut self.nodes[existing].kind {
                *buf = data;
                return Ok(existing);
            }
            return Err("exists and is not a file");
        }
        let id = self.push_node(Node {
            kind: NodeKind::File(data),
            parent,
            owner,
            mode: MODE_FILE_DEFAULT,
        });
        match &mut self.nodes[parent].kind {
            NodeKind::Dir(m) => { m.insert(name.to_string(), id); }
            _ => return Err("parent is not a directory"),
        }
        Ok(id)
    }

    fn mknod(&mut self, cwd: usize, path: &str, kind: NodeKind, owner: u32, mode: u16) -> Result<usize, &'static str> {
        let (parent, name) = self.resolve_parent(cwd, path).ok_or("bad path")?;
        let id = self.push_node(Node { kind, parent, owner, mode });
        match &mut self.nodes[parent].kind {
            NodeKind::Dir(m) => { m.insert(name.to_string(), id); }
            _ => return Err("parent is not a directory"),
        }
        Ok(id)
    }

    pub fn mknod_device(&mut self, cwd: usize, path: &str, dev: u8) -> Result<usize, &'static str> {
        self.mknod(cwd, path, NodeKind::Device(dev), 0, MODE_DEVICE_DEFAULT)
    }

    pub fn mknod_proc(&mut self, cwd: usize, path: &str, generator: fn() -> String) -> Result<usize, &'static str> {
        self.mknod(cwd, path, NodeKind::Proc(generator), 0, MODE_PROC_DEFAULT)
    }

    pub fn read(&self, cwd: usize, path: &str) -> Result<Vec<u8>, &'static str> {
        let id = self.resolve(cwd, path).ok_or("no such file or directory")?;
        match &self.nodes[id].kind {
            NodeKind::File(data) => Ok(data.clone()),
            NodeKind::Proc(generator) => Ok(generator().into_bytes()),
            NodeKind::Device(DEV_ZERO) => Ok(alloc::vec![0u8; 256]),
            NodeKind::Device(_) => Ok(Vec::new()),
            NodeKind::Dir(_) => Err("is a directory"),
        }
    }

    pub fn write(&mut self, cwd: usize, path: &str, data: &[u8], append: bool, uid: u32) -> Result<(), &'static str> {
        let id = match self.resolve(cwd, path) {
            Some(id) => id,
            None => return self.create_file(cwd, path, data.to_vec(), uid).map(|_| ()),
        };
        if !self.can_write(id, uid) {
            return Err("permission denied");
        }
        match &mut self.nodes[id].kind {
            NodeKind::File(buf) => {
                if append {
                    buf.extend_from_slice(data);
                } else {
                    *buf = data.to_vec();
                }
                Ok(())
            }
            NodeKind::Device(_) => Ok(()),
            _ => Err("cannot write to this node"),
        }
    }

    pub fn list(&self, cwd: usize, path: &str) -> Result<Vec<(String, bool)>, &'static str> {
        let id = if path.is_empty() {
            cwd
        } else {
            self.resolve(cwd, path).ok_or("no such file or directory")?
        };
        match &self.nodes[id].kind {
            NodeKind::Dir(m) => Ok(m
                .iter()
                .map(|(n, &cid)| (n.clone(), matches!(self.nodes[cid].kind, NodeKind::Dir(_))))
                .collect()),
            _ => Err("not a directory"),
        }
    }

    pub fn remove(&mut self, cwd: usize, path: &str, uid: u32) -> Result<(), &'static str> {
        let (parent, name) = self.resolve_parent(cwd, path).ok_or("bad path")?;
        let target = self.child(parent, name).ok_or("no such file or directory")?;
        if !self.can_write(parent, uid) && !self.can_write(target, uid) {
            return Err("permission denied");
        }
        match &mut self.nodes[parent].kind {
            NodeKind::Dir(m) => { m.remove(name).ok_or("no such file or directory")?; Ok(()) }
            _ => Err("parent is not a directory"),
        }
    }

    pub fn is_dir(&self, id: usize) -> bool {
        matches!(self.nodes[id].kind, NodeKind::Dir(_))
    }

    pub fn exists(&self, cwd: usize, path: &str) -> bool {
        self.resolve(cwd, path).is_some()
    }

    fn load_tar(&mut self, archive: &[u8]) {
        let root = self.root_id();
        let entries = tar::parse(archive);
        for entry in entries {
            let path = format!("/{}", entry.name.trim_end_matches('/'));
            if entry.is_dir {
                let _ = self.mkdir_all(&path, 0);
            } else {
                if let Some(idx) = path.rfind('/') {
                    let parent_path = &path[..idx.max(1)];
                    let _ = self.mkdir_all(parent_path, 0);
                }
                let _ = self.create_file(root, &path, entry.data.to_vec(), 0);
            }
        }
    }
}

pub static VFS: Mutex<Option<Vfs>> = Mutex::new(None);
pub static DISK_IMAGE: Mutex<Option<(usize, usize)>> = Mutex::new(None);

pub fn disk_image_slice() -> Option<&'static [u8]> {
    let (addr, size) = (*DISK_IMAGE.lock())?;
    if addr == 0 || size == 0 {
        return None;
    }
    Some(unsafe { core::slice::from_raw_parts(addr as *const u8, size) })
}

pub fn root_id() -> usize {
    0
}

pub fn init() {
    let mut vfs = Vfs::new();
    let root = vfs.root_id();

    for d in [
        "bin", "sbin", "etc", "dev", "proc", "sys", "tmp", "var", "usr", "home", "root", "lib",
        "mnt", "media", "opt", "srv", "boot", "run",
    ] {
        let _ = vfs.mkdir(root, &format!("/{}", d), 0);
    }
    for d in [
        "usr/bin", "usr/sbin", "usr/lib", "usr/share", "var/log", "var/tmp", "var/run",
        "home/user", "etc/init.d",
    ] {
        let _ = vfs.mkdir_all(&format!("/{}", d), 0);
    }

    let _ = vfs.create_file(root, "/etc/hostname", b"hamix\n".to_vec(), 0);
    let _ = vfs.create_file(
        root,
        "/etc/passwd",
        b"root:x:0:0:root:/root:/bin/hsh\nuser:x:1000:1000:user:/home/user:/bin/hsh\n".to_vec(),
        0,
    );

    let root_shadow = crate::users::make_shadow_entry("hamix", 0x9f31a2c1);
    let user_shadow = crate::users::make_shadow_entry("user", 0x1b4d7e93);
    let shadow_text = format!("root:{}\nuser:{}\n", root_shadow, user_shadow);
    let _ = vfs.create_file(root, "/etc/shadow", shadow_text.into_bytes(), 0);
    let _ = vfs.chmod(root, "/etc/shadow", 0, 0o600);

    let _ = vfs.create_file(root, "/etc/sudoers", b"user\n".to_vec(), 0);
    let _ = vfs.chmod(root, "/etc/sudoers", 0, 0o600);

    let _ = vfs.create_file(
        root,
        "/etc/os-release",
        b"NAME=\"HamixOS\"\nID=hamix\nVERSION=\"0.1.0\"\nPRETTY_NAME=\"HamixOS 0.1.0\"\n".to_vec(),
        0,
    );
    let _ = vfs.create_file(root, "/etc/motd", b"Welcome to HamixOS.\n".to_vec(), 0);

    let _ = vfs.mknod_device(root, "/dev/null", DEV_NULL);
    let _ = vfs.mknod_device(root, "/dev/zero", DEV_ZERO);
    let _ = vfs.mknod_device(root, "/dev/console", DEV_CONSOLE);
    let _ = vfs.mknod_device(root, "/dev/random", DEV_RANDOM);

    let _ = vfs.mknod_proc(root, "/proc/uptime", proc_uptime);
    let _ = vfs.mknod_proc(root, "/proc/meminfo", proc_meminfo);
    let _ = vfs.mknod_proc(root, "/proc/version", proc_version);
    let _ = vfs.mknod_proc(root, "/proc/cpuinfo", proc_cpuinfo);

    *VFS.lock() = Some(vfs);
}

pub fn load_initramfs(addr: usize, size: usize) {
    if addr == 0 || size == 0 {
        return;
    }
    let data = unsafe { core::slice::from_raw_parts(addr as *const u8, size) };
    if let Some(vfs) = VFS.lock().as_mut() {
        vfs.load_tar(data);
    }
}

fn proc_uptime() -> String {
    let ticks = crate::task::uptime_ticks();
    format!("{}.{:02}\n", ticks / 100, ticks % 100)
}

fn proc_meminfo() -> String {
    let (free, total) = crate::memory::frame::memory_info();
    format!(
        "MemTotal:   {:>10} kB\nMemFree:    {:>10} kB\n",
        total / 1024,
        free / 1024
    )
}

fn proc_version() -> String {
    String::from("HamixOS version 0.1.0 (rustc nightly, no_std) #1 x86_64\n")
}

fn proc_cpuinfo() -> String {
    String::from(
        "vendor_id\t: GenuineIntel\nmodel name\t: Intel Celeron T3100 / Pentium G640 compatible\narchitecture\t: x86_64\n",
    )
}
