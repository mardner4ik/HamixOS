pub mod tar;
pub mod hextfs;

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use spin::Mutex;

pub const DEV_NULL: u8 = 0;
pub const DEV_ZERO: u8 = 1;
pub const DEV_CONSOLE: u8 = 2;
pub const DEV_RANDOM: u8 = 3;
pub const DEV_FB0: u8 = 4;
pub const DEV_BLOCK: u8 = 5;

pub const MODE_DIR_DEFAULT: u16 = 0o755;
pub const MODE_FILE_DEFAULT: u16 = 0o644;
pub const MODE_DEVICE_DEFAULT: u16 = 0o666;
pub const MODE_PROC_DEFAULT: u16 = 0o444;

pub const DIRTY_DATA: u8 = 1;
pub const DIRTY_META: u8 = 2;
pub const VOLATILE: u32 = u32::MAX;
pub const LINK_MAGIC: &[u8] = b"\x7fHXLINK\n";
const LINK_MAX: usize = 4096;
pub const EAGER_FILE_MAX: usize = 8 + LINK_MAX;
const STREAM_MIN: usize = 4 * 1024 * 1024;

pub const KIND_FILE: u32 = 1;
pub const KIND_DIR: u32 = 2;
pub const KIND_DEVICE: u32 = 3;
pub const KIND_PROC: u32 = 4;

pub const NO_SPACE: &str = "no space left on device";

fn reserve_body(bytes: &mut Vec<u8>, end: usize) -> Result<(), &'static str> {
    if end <= bytes.capacity() {
        return Ok(());
    }
    let len = bytes.len();
    let wanted = end.max(len + (len / 8).clamp(4096, 4 << 20));
    if bytes.try_reserve_exact(wanted - len).is_ok() || bytes.try_reserve_exact(end - len).is_ok() {
        Ok(())
    } else {
        Err(NO_SPACE)
    }
}

pub struct FileBody {
    bytes: Vec<u8>,
    size: usize,
    resident: bool,
    backing: u32,
    used: u64,
}

impl FileBody {
    pub fn held(data: Vec<u8>) -> FileBody {
        FileBody { size: data.len(), bytes: data, resident: true, backing: 0, used: 0 }
    }

    pub fn backed(size: usize, backing: u32) -> FileBody {
        FileBody { bytes: Vec::new(), size, resident: false, backing, used: 0 }
    }

    pub fn len(&self) -> usize {
        if self.resident { self.bytes.len() } else { self.size }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn evictable(&self) -> bool {
        self.resident && self.backing != 0 && self.bytes.len() > EAGER_FILE_MAX
    }

    pub fn is_resident(&self) -> bool {
        self.resident
    }

    fn cached_len(&self) -> usize {
        if self.resident && self.backing != 0 { self.bytes.len() } else { 0 }
    }

    fn drop_cache(&mut self) -> usize {
        let freed = self.bytes.len();
        self.size = freed;
        self.bytes = Vec::new();
        self.resident = false;
        freed
    }

    fn adopt(&mut self, data: Vec<u8>) {
        self.size = data.len();
        self.bytes = data;
        self.resident = true;
    }

    fn detach(&mut self) {
        self.backing = 0;
    }

    pub fn snapshot(&self) -> Vec<u8> {
        self.bytes.clone()
    }

    pub fn differs(&self, other: &[u8]) -> bool {
        !self.resident || self.bytes != other
    }

    pub fn replace_from_overlay(&mut self, data: Vec<u8>) {
        self.adopt(data);
        self.detach();
    }

    pub fn discard(&mut self) {
        self.bytes = Vec::new();
        self.size = 0;
        self.resident = true;
        self.backing = 0;
    }
}

pub enum NodeKind {
    Dir(BTreeMap<String, usize>),
    File(FileBody),
    Device(u8),
    Proc(fn() -> String),
}

pub struct Node {
    kind: NodeKind,
    parent: usize,
    owner: u32,
    mode: u16,
    ino: u32,
    dirty: u8,
    dev: u16,
}

pub struct Stat {
    pub kind: u32,
    pub mode: u16,
    pub owner: u32,
    pub size: u64,
    pub dev: u16,
}

static FILE_CACHE_BYTES: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
static VFS_NODE_COUNT: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
static LAST_FILE_LOAD_MS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

pub fn file_cache_bytes() -> usize {
    FILE_CACHE_BYTES.load(core::sync::atomic::Ordering::Relaxed)
}

pub fn vfs_node_count() -> usize {
    VFS_NODE_COUNT.load(core::sync::atomic::Ordering::Relaxed)
}

pub fn last_file_load_ms() -> u64 {
    LAST_FILE_LOAD_MS.load(core::sync::atomic::Ordering::Relaxed)
}

pub struct Vfs {
    nodes: Vec<Node>,
    pub tracking: bool,
    removed: Vec<(u16, u32, String)>,
    clock: u64,
    cached: usize,
}

impl Vfs {
    fn new() -> Self {
        let root = Node {
            kind: NodeKind::Dir(BTreeMap::new()),
            parent: 0,
            owner: 0,
            mode: MODE_DIR_DEFAULT,
            ino: 0,
            dirty: 0,
            dev: 0,
        };
        Self { nodes: alloc::vec![root], tracking: false, removed: Vec::new(), clock: 0, cached: 0 }
    }

    fn make_resident(&mut self, id: usize) {
        let (backing, dev) = match &self.nodes[id].kind {
            NodeKind::File(body) if !body.resident && body.backing != 0 => (body.backing, self.nodes[id].dev),
            NodeKind::File(_) => {
                self.clock += 1;
                let stamp = self.clock;
                if let NodeKind::File(body) = &mut self.nodes[id].kind {
                    body.used = stamp;
                }
                return;
            }
            _ => return,
        };
        let data = hextfs::load_backing(dev, backing).unwrap_or_default();
        self.clock += 1;
        let stamp = self.clock;
        self.cached += data.len();
        FILE_CACHE_BYTES.store(self.cached, core::sync::atomic::Ordering::Relaxed);
        LAST_FILE_LOAD_MS.store(crate::task::uptime_ms(), core::sync::atomic::Ordering::Relaxed);
        if let NodeKind::File(body) = &mut self.nodes[id].kind {
            body.adopt(data);
            body.used = stamp;
        }
    }

    pub fn set_backing(&mut self, id: usize, ino: u32) {
        if let NodeKind::File(body) = &mut self.nodes[id].kind {
            if body.backing == 0 && body.resident {
                body.backing = ino;
                self.cached += body.bytes.len();
                FILE_CACHE_BYTES.store(self.cached, core::sync::atomic::Ordering::Relaxed);
            }
        }
    }

    pub fn cached_file_bytes(&self) -> usize {
        self.cached
    }

    pub fn evict_files(&mut self, keep: usize) -> usize {
        self.cached = self
            .nodes
            .iter()
            .map(|node| match &node.kind {
                NodeKind::File(body) => body.cached_len(),
                _ => 0,
            })
            .sum();
        FILE_CACHE_BYTES.store(self.cached, core::sync::atomic::Ordering::Relaxed);
        if self.cached <= keep {
            return 0;
        }
        let mut candidates: Vec<(u64, usize)> = self
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(id, node)| match &node.kind {
                NodeKind::File(body) if body.evictable() && node.dirty == 0 => Some((body.used, id)),
                _ => None,
            })
            .collect();
        candidates.sort_unstable();
        let mut freed = 0;
        for (_, id) in candidates {
            if self.cached.saturating_sub(freed) <= keep {
                break;
            }
            if let NodeKind::File(body) = &mut self.nodes[id].kind {
                freed += body.drop_cache();
            }
        }
        self.cached = self.cached.saturating_sub(freed);
        FILE_CACHE_BYTES.store(self.cached, core::sync::atomic::Ordering::Relaxed);
        freed
    }

    pub fn root_id(&self) -> usize {
        0
    }

    pub fn release_orphans(&mut self, open: &alloc::collections::BTreeSet<usize>, pinned: &[usize], previous: &mut alloc::collections::BTreeSet<usize>) -> usize {
        let mut reachable = alloc::vec![false; self.nodes.len()];
        let mut stack: Vec<usize> = alloc::vec![0];
        stack.extend_from_slice(pinned);
        while let Some(id) = stack.pop() {
            if id >= reachable.len() || reachable[id] {
                continue;
            }
            reachable[id] = true;
            if let NodeKind::Dir(map) = &self.nodes[id].kind {
                stack.extend(map.values().copied());
            }
        }
        let mut candidates = alloc::collections::BTreeSet::new();
        let mut freed = 0;
        for (id, node) in self.nodes.iter_mut().enumerate() {
            if reachable[id] || open.contains(&id) {
                continue;
            }
            match &mut node.kind {
                NodeKind::File(body) if body.bytes.capacity() != 0 || body.backing != 0 || body.size != 0 => {
                    if previous.contains(&id) {
                        freed += body.bytes.len();
                        body.discard();
                        node.dirty = 0;
                    } else {
                        candidates.insert(id);
                    }
                }
                NodeKind::Dir(map) if !map.is_empty() => {
                    if previous.contains(&id) {
                        map.clear();
                    } else {
                        candidates.insert(id);
                    }
                }
                _ => {}
            }
        }
        *previous = candidates;
        freed
    }

    fn push_node(&mut self, node: Node) -> usize {
        self.nodes.push(node);
        VFS_NODE_COUNT.store(self.nodes.len(), core::sync::atomic::Ordering::Relaxed);
        self.nodes.len() - 1
    }

    fn touch(&mut self, id: usize, flags: u8) {
        if self.tracking {
            self.nodes[id].dirty |= flags;
        }
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

    pub fn path_of(&self, mut id: usize) -> String {
        let mut parts: Vec<String> = Vec::new();
        let mut guard = 0;
        while id != 0 && guard < 256 {
            let parent = self.nodes[id].parent;
            if let NodeKind::Dir(map) = &self.nodes[parent].kind {
                if let Some((name, _)) = map.iter().find(|(_, v)| **v == id) {
                    parts.push(name.clone());
                }
            }
            id = parent;
            guard += 1;
        }
        parts.reverse();
        format!("/{}", parts.join("/"))
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

    pub fn can_read(&self, id: usize, uid: u32) -> bool {
        if uid == 0 {
            return true;
        }
        let node = &self.nodes[id];
        if node.owner == uid {
            return node.mode & 0o400 != 0;
        }
        node.mode & 0o004 != 0
    }

    pub fn stat(&self, id: usize) -> Stat {
        let node = &self.nodes[id];
        let kind = match node.kind {
            NodeKind::Dir(_) => KIND_DIR,
            NodeKind::File(_) => KIND_FILE,
            NodeKind::Device(_) => KIND_DEVICE,
            NodeKind::Proc(_) => KIND_PROC,
        };
        Stat { kind, mode: node.mode, owner: node.owner, size: self.node_size(id) as u64, dev: node.dev }
    }

    pub fn chmod(&mut self, cwd: usize, path: &str, uid: u32, mode: u16) -> Result<(), &'static str> {
        let id = self.resolve(cwd, path).ok_or("no such file or directory")?;
        if uid != 0 && self.nodes[id].owner != uid {
            return Err("permission denied");
        }
        self.nodes[id].mode = mode & 0o7777;
        self.touch(id, DIRTY_META);
        Ok(())
    }

    pub fn chown(&mut self, cwd: usize, path: &str, uid: u32, new_owner: u32) -> Result<(), &'static str> {
        if uid != 0 {
            return Err("permission denied");
        }
        let id = self.resolve(cwd, path).ok_or("no such file or directory")?;
        self.nodes[id].owner = new_owner;
        self.touch(id, DIRTY_META);
        Ok(())
    }

    fn attach_child(&mut self, parent: usize, name: &str, id: usize) -> Result<(), &'static str> {
        match &mut self.nodes[parent].kind {
            NodeKind::Dir(m) => {
                m.insert(name.to_string(), id);
                Ok(())
            }
            _ => Err("parent is not a directory"),
        }
    }

    pub fn mkdir(&mut self, cwd: usize, path: &str, owner: u32) -> Result<usize, &'static str> {
        let (parent, name) = self.resolve_parent(cwd, path).ok_or("bad path")?;
        if !self.is_dir(parent) {
            return Err("parent is not a directory");
        }
        if !self.can_write(parent, owner) {
            return Err("permission denied");
        }
        if self.child(parent, name).is_some() {
            return Err("already exists");
        }
        let dev = self.nodes[parent].dev;
        let id = self.push_node(Node {
            kind: NodeKind::Dir(BTreeMap::new()),
            parent,
            owner,
            mode: MODE_DIR_DEFAULT,
            ino: 0,
            dirty: DIRTY_META,
            dev,
        });
        self.attach_child(parent, name, id)?;
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
        if !self.is_dir(parent) {
            return Err("parent is not a directory");
        }
        if let Some(existing) = self.child(parent, name) {
            if !self.can_write(existing, owner) {
                return Err("permission denied");
            }
            if let NodeKind::File(body) = &mut self.nodes[existing].kind {
                body.adopt(data);
                body.detach();
                self.touch(existing, DIRTY_DATA);
                return Ok(existing);
            }
            return Err("exists and is not a file");
        }
        if !self.can_write(parent, owner) {
            return Err("permission denied");
        }
        let dev = self.nodes[parent].dev;
        let id = self.push_node(Node {
            kind: NodeKind::File(FileBody::held(data)),
            parent,
            owner,
            mode: MODE_FILE_DEFAULT,
            ino: 0,
            dirty: DIRTY_DATA | DIRTY_META,
            dev,
        });
        self.attach_child(parent, name, id)?;
        Ok(id)
    }

    fn mknod(&mut self, cwd: usize, path: &str, kind: NodeKind, owner: u32, mode: u16) -> Result<usize, &'static str> {
        let (parent, name) = self.resolve_parent(cwd, path).ok_or("bad path")?;
        let dev = self.nodes[parent].dev;
        let id = self.push_node(Node { kind, parent, owner, mode, ino: VOLATILE, dirty: 0, dev });
        self.attach_child(parent, name, id)?;
        Ok(id)
    }

    pub fn mknod_device(&mut self, cwd: usize, path: &str, dev: u8) -> Result<usize, &'static str> {
        let mode = if dev == DEV_BLOCK { 0o660 } else { MODE_DEVICE_DEFAULT };
        self.mknod(cwd, path, NodeKind::Device(dev), 0, mode)
    }

    pub fn mknod_proc(&mut self, cwd: usize, path: &str, generator: fn() -> String) -> Result<usize, &'static str> {
        self.mknod(cwd, path, NodeKind::Proc(generator), 0, MODE_PROC_DEFAULT)
    }

    pub fn read(&mut self, cwd: usize, path: &str) -> Result<Vec<u8>, &'static str> {
        let id = self.resolve(cwd, path).ok_or("no such file or directory")?;
        self.make_resident(id);
        match &self.nodes[id].kind {
            NodeKind::File(body) => Ok(body.bytes.clone()),
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
        let tracking = self.tracking;
        if append {
            self.make_resident(id);
        }
        match &mut self.nodes[id].kind {
            NodeKind::File(body) => {
                if append {
                    body.bytes.extend_from_slice(data);
                    body.size = body.bytes.len();
                } else {
                    body.adopt(data.to_vec());
                }
                body.detach();
                if tracking {
                    self.nodes[id].dirty |= DIRTY_DATA;
                }
                Ok(())
            }
            NodeKind::Device(_) => Ok(()),
            _ => Err("cannot write to this node"),
        }
    }

    pub fn list(&self, cwd: usize, path: &str) -> Result<Vec<(String, bool)>, &'static str> {
        let id = if path.is_empty() { cwd } else { self.resolve(cwd, path).ok_or("no such file or directory")? };
        match &self.nodes[id].kind {
            NodeKind::Dir(m) => Ok(m.iter().map(|(n, &cid)| (n.clone(), matches!(self.nodes[cid].kind, NodeKind::Dir(_)))).collect()),
            _ => Err("not a directory"),
        }
    }

    fn record_removal(&mut self, parent: usize, target: usize, name: &str) {
        let parent_ino = self.nodes[parent].ino;
        let target_ino = self.nodes[target].ino;
        let dev = self.nodes[target].dev;
        if self.tracking
            && self.nodes[parent].dev == dev
            && parent_ino != 0
            && parent_ino != VOLATILE
            && target_ino != 0
            && target_ino != VOLATILE
        {
            self.removed.push((dev, parent_ino, name.to_string()));
        }
    }

    pub fn remove(&mut self, cwd: usize, path: &str, uid: u32) -> Result<(), &'static str> {
        let (parent, name) = self.resolve_parent(cwd, path).ok_or("bad path")?;
        let target = self.child(parent, name).ok_or("no such file or directory")?;
        if !self.can_write(parent, uid) {
            return Err("permission denied");
        }
        if hextfs::is_mount_point(target) {
            return Err("device or resource busy");
        }
        self.record_removal(parent, target, name);
        match &mut self.nodes[parent].kind {
            NodeKind::Dir(m) => {
                m.remove(name).ok_or("no such file or directory")?;
            }
            _ => return Err("parent is not a directory"),
        }
        Ok(())
    }

    fn mark_fresh(&mut self, id: usize, dev: u16) {
        self.make_resident(id);
        if self.nodes[id].ino != VOLATILE {
            self.nodes[id].ino = 0;
            self.nodes[id].dirty = DIRTY_DATA | DIRTY_META;
        }
        self.nodes[id].dev = dev;
        let children: Vec<usize> = match &self.nodes[id].kind {
            NodeKind::Dir(m) => m.values().copied().collect(),
            _ => Vec::new(),
        };
        for child in children {
            if !hextfs::is_mount_point(child) {
                self.mark_fresh(child, dev);
            }
        }
    }

    pub fn rename(&mut self, cwd: usize, from: &str, to: &str, uid: u32) -> Result<(), &'static str> {
        let (old_parent, old_name) = self.resolve_parent(cwd, from).ok_or("bad path")?;
        let target = self.child(old_parent, old_name).ok_or("no such file or directory")?;
        if hextfs::is_mount_point(target) {
            return Err("device or resource busy");
        }
        let (mut new_parent, mut new_name) = self.resolve_parent(cwd, to).ok_or("bad path")?;
        let owned_name;
        if let Some(existing) = self.child(new_parent, new_name) {
            if self.is_dir(existing) {
                new_parent = existing;
                owned_name = old_name.to_string();
                new_name = owned_name.as_str();
            }
        }
        if !self.is_dir(new_parent) {
            return Err("destination is not a directory");
        }
        if !self.can_write(old_parent, uid) || !self.can_write(new_parent, uid) {
            return Err("permission denied");
        }
        let mut probe = new_parent;
        for _ in 0..512 {
            if probe == target {
                return Err("cannot move a directory into itself");
            }
            if probe == 0 {
                break;
            }
            probe = self.nodes[probe].parent;
        }
        if new_parent == old_parent && new_name == old_name {
            return Ok(());
        }
        let new_name = new_name.to_string();
        if let Some(existing) = self.child(new_parent, &new_name) {
            if self.is_dir(existing) {
                return Err("destination exists");
            }
            self.record_removal(new_parent, existing, &new_name);
        }
        self.record_removal(old_parent, target, old_name);
        if let NodeKind::Dir(m) = &mut self.nodes[old_parent].kind {
            m.remove(old_name);
        }
        self.attach_child(new_parent, &new_name, target)?;
        self.nodes[target].parent = new_parent;
        let dev = self.nodes[new_parent].dev;
        self.mark_fresh(target, dev);
        Ok(())
    }

    pub fn node_size(&self, id: usize) -> usize {
        match &self.nodes[id].kind {
            NodeKind::File(body) => body.len(),
            NodeKind::Proc(generator) => generator().len(),
            _ => 0,
        }
    }

    pub fn read_node_at(&mut self, id: usize, pos: usize, buf: &mut [u8]) -> Result<usize, &'static str> {
        let streamed = match &self.nodes[id].kind {
            NodeKind::File(body) if !body.resident && body.backing != 0 && body.size > STREAM_MIN => Some((body.backing, body.size)),
            _ => None,
        };
        if let Some((backing, size)) = streamed {
            if pos >= size {
                return Ok(0);
            }
            let want = buf.len().min(size - pos);
            let dev = self.nodes[id].dev;
            return hextfs::load_range(dev, backing, pos as u64, &mut buf[..want]).ok_or("input/output error");
        }
        if matches!(self.nodes[id].kind, NodeKind::File(_)) {
            self.make_resident(id);
        }
        match &self.nodes[id].kind {
            NodeKind::File(body) => {
                let data = &body.bytes;
                if pos >= data.len() {
                    return Ok(0);
                }
                let n = (data.len() - pos).min(buf.len());
                buf[..n].copy_from_slice(&data[pos..pos + n]);
                Ok(n)
            }
            NodeKind::Proc(generator) => {
                let data = generator().into_bytes();
                if pos >= data.len() {
                    return Ok(0);
                }
                let n = (data.len() - pos).min(buf.len());
                buf[..n].copy_from_slice(&data[pos..pos + n]);
                Ok(n)
            }
            NodeKind::Device(DEV_ZERO) => {
                buf.fill(0);
                Ok(buf.len())
            }
            NodeKind::Device(DEV_RANDOM) => {
                crate::random::fill(buf);
                Ok(buf.len())
            }
            NodeKind::Device(_) => Ok(0),
            NodeKind::Dir(_) => Err("is a directory"),
        }
    }

    pub fn write_node_at(&mut self, id: usize, pos: usize, data: &[u8], uid: u32) -> Result<(), &'static str> {
        if !self.can_write(id, uid) {
            return Err("permission denied");
        }
        self.make_resident(id);
        self.touch(id, DIRTY_DATA);
        match &mut self.nodes[id].kind {
            NodeKind::File(body) => {
                let buf = &mut body.bytes;
                reserve_body(buf, pos.max(pos + data.len()))?;
                if buf.len() < pos {
                    buf.resize(pos, 0);
                }
                let overlap = (buf.len() - pos).min(data.len());
                buf[pos..pos + overlap].copy_from_slice(&data[..overlap]);
                buf.extend_from_slice(&data[overlap..]);
                body.size = buf.len();
                body.detach();
                Ok(())
            }
            NodeKind::Device(_) => Ok(()),
            _ => Err("cannot write to this node"),
        }
    }

    pub fn set_len(&mut self, id: usize, len: usize, uid: u32) -> Result<(), &'static str> {
        if !self.can_write(id, uid) {
            return Err("permission denied");
        }
        self.make_resident(id);
        self.touch(id, DIRTY_DATA);
        match &mut self.nodes[id].kind {
            NodeKind::File(body) => {
                if len < body.bytes.len() / 2 {
                    body.bytes.truncate(len);
                    body.bytes.shrink_to_fit();
                } else {
                    reserve_body(&mut body.bytes, len)?;
                }
                body.bytes.resize(len, 0);
                body.size = len;
                body.detach();
                Ok(())
            }
            NodeKind::Device(_) => Ok(()),
            _ => Err("not a regular file"),
        }
    }

    pub fn link_target(&mut self, id: usize) -> Option<String> {
        let size = match &self.nodes[id].kind {
            NodeKind::File(body) => body.len(),
            _ => return None,
        };
        if size <= LINK_MAGIC.len() || size > LINK_MAGIC.len() + LINK_MAX {
            return None;
        }
        self.make_resident(id);
        self.peek_link(id)
    }

    pub fn peek_link(&self, id: usize) -> Option<String> {
        match &self.nodes[id].kind {
            NodeKind::File(body) if body.resident && body.bytes.len() > LINK_MAGIC.len() && body.bytes.len() <= LINK_MAGIC.len() + LINK_MAX && body.bytes.starts_with(LINK_MAGIC) => {
                Some(String::from_utf8_lossy(&body.bytes[LINK_MAGIC.len()..]).into_owned())
            }
            _ => None,
        }
    }

    pub fn dir_is_empty(&self, id: usize) -> bool {
        match &self.nodes[id].kind {
            NodeKind::Dir(m) => m.is_empty(),
            _ => false,
        }
    }

    pub fn truncate_node(&mut self, id: usize, uid: u32) -> Result<(), &'static str> {
        if !self.can_write(id, uid) {
            return Err("permission denied");
        }
        self.touch(id, DIRTY_DATA);
        match &mut self.nodes[id].kind {
            NodeKind::File(body) => {
                body.adopt(Vec::new());
                body.detach();
                Ok(())
            }
            NodeKind::Device(_) => Ok(()),
            _ => Err("not a regular file"),
        }
    }

    pub fn list_detailed(&self, cwd: usize, path: &str) -> Result<Vec<(String, usize)>, &'static str> {
        let id = if path.is_empty() { cwd } else { self.resolve(cwd, path).ok_or("no such file or directory")? };
        match &self.nodes[id].kind {
            NodeKind::Dir(m) => Ok(m.iter().map(|(n, &cid)| (n.clone(), cid)).collect()),
            _ => Err("not a directory"),
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
        for entry in tar::parse(archive) {
            let path = format!("/{}", entry.name.trim_end_matches('/'));
            if entry.is_dir {
                let _ = self.mkdir_all(&path, 0);
            } else {
                if let Some(idx) = path.rfind('/') {
                    let _ = self.mkdir_all(&path[..idx.max(1)], 0);
                }
                let _ = self.create_file(root, &path, entry.data.to_vec(), 0);
            }
        }
    }
}

pub struct VfsLock(Mutex<Option<Vfs>>);

impl VfsLock {
    const fn new() -> Self {
        VfsLock(Mutex::new(None))
    }

    pub fn lock(&self) -> spin::MutexGuard<'_, Option<Vfs>> {
        loop {
            if let Some(guard) = self.0.try_lock() {
                return guard;
            }
            if crate::arch::interrupts_enabled() && crate::task::current_pid() != 0 {
                crate::task::yield_now();
            } else {
                core::hint::spin_loop();
            }
        }
    }

    pub fn try_lock(&self) -> Option<spin::MutexGuard<'_, Option<Vfs>>> {
        self.0.try_lock()
    }
}

pub static VFS: VfsLock = VfsLock::new();

pub fn absolute(cwd: &str, path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    let mut room = path.len() + 1;
    if !path.starts_with('/') {
        parts.extend(cwd.split('/').filter(|p| !p.is_empty()));
        room += cwd.len();
    }
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    let mut out = String::with_capacity(room);
    out.push('/');
    for (index, part) in parts.iter().enumerate() {
        if index > 0 {
            out.push('/');
        }
        out.push_str(part);
    }
    out
}

pub fn request_sync() {
    hextfs::request_sync();
}

pub fn sync() {
    hextfs::sync();
}

pub fn root_id() -> usize {
    0
}

pub fn init() {
    let mut vfs = Vfs::new();
    let root = vfs.root_id();

    for d in [
        "bin", "sbin", "etc", "dev", "proc", "sys", "tmp", "var", "usr", "home", "root", "lib", "mnt", "media", "opt",
        "srv", "boot", "run",
    ] {
        let _ = vfs.mkdir(root, &format!("/{}", d), 0);
    }
    for d in ["usr/bin", "usr/sbin", "usr/lib", "usr/share", "var/log", "var/tmp", "var/run", "home/user", "etc/hamix"] {
        let _ = vfs.mkdir_all(&format!("/{}", d), 0);
    }
    let _ = vfs.chmod(root, "/tmp", 0, 0o1777);

    let _ = vfs.create_file(root, "/etc/hostname", b"hamix\n".to_vec(), 0);
    let _ = vfs.create_file(
        root,
        "/etc/passwd",
        b"root:x:0:0:root:/root:/usr/bin/hsh\nuser:x:1000:1000:user:/home/user:/usr/bin/hsh\n".to_vec(),
        0,
    );
    let shadow_text = format!(
        "root:{}\nuser:{}\n",
        crate::users::make_shadow_entry("hamix", 0x9f31a2c1),
        crate::users::make_shadow_entry("user", 0x1b4d7e93)
    );
    let _ = vfs.create_file(root, "/etc/shadow", shadow_text.into_bytes(), 0);
    let _ = vfs.chmod(root, "/etc/shadow", 0, 0o600);
    let _ = vfs.create_file(root, "/etc/sudoers", b"user\n".to_vec(), 0);
    let _ = vfs.chmod(root, "/etc/sudoers", 0, 0o600);
    let _ = vfs.create_file(root, "/etc/hamix/login.conf", b"shell=/usr/bin/hsh\ndesktop=/usr/bin/hxserver\nautostart_desktop=no\n".to_vec(), 0);

    *VFS.lock() = Some(vfs);
}

pub fn create_volatile_nodes() {
    let mut guard = VFS.lock();
    let Some(vfs) = guard.as_mut() else {
        return;
    };
    let root = vfs.root_id();
    let tracking = vfs.tracking;
    vfs.tracking = false;
    for (path, dev) in [
        ("/dev/null", DEV_NULL),
        ("/dev/zero", DEV_ZERO),
        ("/dev/console", DEV_CONSOLE),
        ("/dev/random", DEV_RANDOM),
        ("/dev/urandom", DEV_RANDOM),
        ("/dev/fb0", DEV_FB0),
    ] {
        if !vfs.exists(root, path) {
            let _ = vfs.mknod_device(root, path, dev);
        }
    }
    for (path, value) in [
        ("/proc/sys/fs/inotify/max_user_watches", "524288\n"),
        ("/proc/sys/fs/inotify/max_user_instances", "128\n"),
        ("/proc/sys/fs/inotify/max_queued_events", "16384\n"),
        ("/proc/sys/fs/file-max", "65536\n"),
        ("/proc/sys/kernel/pid_max", "32768\n"),
        ("/proc/sys/kernel/threads-max", "4096\n"),
        ("/proc/sys/kernel/osrelease", "6.1.0-hamix\n"),
        ("/proc/sys/vm/overcommit_memory", "0\n"),
        ("/proc/sys/vm/max_map_count", "65530\n"),
    ] {
        if !vfs.exists(root, path) {
            let _ = vfs.write(0, path, value.as_bytes(), false, 0);
        }
    }
    for dir in ["/dev/shm", "/tmp"] {
        if !vfs.exists(root, dir) {
            let _ = vfs.mkdir_all(dir, 0);
        }
        let _ = vfs.chmod(root, dir, 0, 0o1777);
    }
    if !vfs.exists(root, "/proc/self") {
        let _ = vfs.mknod(root, "/proc/self", NodeKind::Dir(BTreeMap::new()), 0, MODE_DIR_DEFAULT);
    }
    let procs: [(&str, fn() -> String); 20] = [
        ("/proc/interrupts", proc_interrupts),
        ("/proc/modules_detail", proc_modules_detail),
        ("/proc/uptime", proc_uptime),
        ("/proc/meminfo", proc_meminfo),
        ("/proc/version", proc_version),
        ("/proc/cpuinfo", proc_cpuinfo),
        ("/proc/dmesg", proc_dmesg),
        ("/proc/mounts", proc_mounts),
        ("/proc/partitions", proc_partitions),
        ("/proc/usb", proc_usb),
        ("/proc/drivers", proc_drivers),
        ("/proc/modules", proc_modules),
        ("/proc/gpu", proc_gpu),
        ("/proc/mouse", proc_mouse),
        ("/proc/cmdline", proc_cmdline),
        ("/proc/stat", proc_stat),
        ("/proc/hxinit", proc_hxinit),
        ("/proc/audio", proc_audio),
        ("/proc/self/exe", proc_self_exe),
        ("/proc/self/maps", proc_self_maps),
    ];
    for (path, generator) in procs {
        if !vfs.exists(root, path) {
            let _ = vfs.mknod_proc(root, path, generator);
        }
    }
    vfs.tracking = tracking;
    drop(guard);
    refresh_block_nodes();
}

pub fn refresh_block_nodes() {
    let volumes = crate::drivers::block::volumes();
    let mut guard = VFS.lock();
    let Some(vfs) = guard.as_mut() else {
        return;
    };
    let Some(dev_dir) = vfs.resolve(0, "/dev") else {
        return;
    };
    let stale: Vec<String> = match &vfs.nodes[dev_dir].kind {
        NodeKind::Dir(map) => map
            .iter()
            .filter(|(name, id)| name.starts_with("sd") && matches!(vfs.nodes[**id].kind, NodeKind::Device(DEV_BLOCK)))
            .map(|(name, _)| name.clone())
            .collect(),
        _ => Vec::new(),
    };
    if let NodeKind::Dir(map) = &mut vfs.nodes[dev_dir].kind {
        for name in stale {
            map.remove(&name);
        }
    }
    for volume in volumes {
        let _ = vfs.mknod_device(0, &format!("/dev/{}", volume.name), DEV_BLOCK);
    }
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

fn proc_self_exe() -> String {
    crate::task::with_current(|t| t.exe.clone())
}

fn proc_self_maps() -> String {
    let (brk_start, brk, mmap_base, mmap_next, exe) = crate::task::with_current(|t| (t.brk_start, t.brk, crate::arch::paging::USER_MMAP_BASE, t.mmap_next, t.exe.clone()));
    if exe.is_empty() {
        return String::new();
    }
    let stack_top = crate::arch::paging::USER_STACK_TOP;
    let mut out = String::new();
    if brk > brk_start {
        out.push_str(&format!("{:x}-{:x} rw-p 00000000 00:00 0                          [heap]\n", brk_start, (brk + 0xFFF) & !0xFFF));
    }
    if mmap_next > mmap_base {
        out.push_str(&format!("{:x}-{:x} rw-p 00000000 00:00 0\n", mmap_base, mmap_next));
    }
    out.push_str(&format!("{:x}-{:x} rw-p 00000000 00:00 0                          [stack]\n", stack_top - 0x10_0000, stack_top));
    out
}

fn proc_uptime() -> String {
    let ms = crate::task::uptime_ms();
    format!("{}.{:02}\n", ms / 1000, (ms % 1000) / 10)
}

fn proc_meminfo() -> String {
    let (free, total) = crate::memory::frame::memory_info();
    let (heap_free, heap_total) = crate::memory::heap_stats();
    let cached = file_cache_bytes();
    let (vmalloc_bytes, vmalloc_blocks) = crate::memory::vmalloc::stats();
    format!(
        "MemTotal:   {:>10} kB\nMemFree:    {:>10} kB\nMemUsed:    {:>10} kB\nKernelHeap: {:>10} kB\nHeapFree:   {:>10} kB\nVmalloc:    {:>10} kB\nVmallocBlk: {:>10}\nFileCache:  {:>10} kB\nVfsNodes:   {:>10}\n",
        total / 1024,
        free / 1024,
        (total - free) / 1024,
        heap_total / 1024,
        heap_free / 1024,
        vmalloc_bytes / 1024,
        vmalloc_blocks,
        cached / 1024,
        vfs_node_count()
    )
}

fn proc_version() -> String {
    format!("HamixOS version 0.6.1 (rustc nightly, no_std) #1 {}\n", crate::arch::MACHINE)
}

fn proc_cpuinfo() -> String {
    crate::arch::platform::cpuinfo_text()
}

fn proc_dmesg() -> String {
    let mut out = String::new();
    crate::drivers::klog::for_each(|line| {
        out.push_str(line);
        out.push('\n');
    });
    out
}

fn proc_mounts() -> String {
    hextfs::mounts_text()
}

fn proc_partitions() -> String {
    let mut out = String::new();
    for volume in crate::drivers::block::volumes() {
        out.push_str(&format!("{}\t{}\t{}\n", volume.name, volume.offset, volume.sectors));
    }
    out
}

fn proc_usb() -> String {
    crate::drivers::usb::describe()
}

fn proc_audio() -> String {
    crate::drivers::audio::info_text()
}

fn proc_drivers() -> String {
    let mut out = String::new();
    crate::drivers::video::registry::for_each(|driver| {
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\n",
            driver.name(),
            driver.version(),
            driver.kind(),
            if driver.is_ready() { "ready" } else { "not ready" }
        ));
    });
    out
}

fn proc_modules() -> String {
    let mut out = format!(
        "budget\t{}\t{}\nsymbols\t{}\n",
        crate::module::resident_bytes(),
        crate::module::BUDGET_BYTES,
        crate::module::symbols::count()
    );
    for info in crate::module::loaded() {
        out.push_str(&format!("module\t{}\t{}\t{}\t{}\t{}\n", info.name, info.bytes, info.text_bytes, info.data_bytes, info.devices));
    }
    for (module, class, name, vendor, device) in crate::module::claims() {
        out.push_str(&format!("device\t{}\t{}\t{}\t{:04x}:{:04x}\n", module, class, name, vendor, device));
    }
    out
}

fn proc_interrupts() -> String {
    let (spurious, worked, dropped) = crate::drivers::irq::stats();
    let mut out = String::new();
    for (vector, owner, count, legacy) in crate::drivers::irq::lines() {
        out.push_str(&format!("{}\t{}\t{}\t{}\n", vector, if legacy { "pic" } else { "msi" }, count, owner));
    }
    out.push_str(&format!("timer\t{}\n", crate::task::ticks()));
    out.push_str(&format!("spurious\t{}\nwork\t{}\ndropped\t{}\n", spurious, worked, dropped));
    out
}

fn proc_modules_detail() -> String {
    let mut out = String::new();
    for info in crate::module::loaded() {
        out.push_str(&format!(
            "module\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            info.name,
            info.version,
            info.allocated,
            info.limit,
            info.signature,
            if info.protected { "wx" } else { "rwx" },
            if info.stalled { "stalled" } else if info.suspended { "suspended" } else { "live" },
            info.interrupts,
            info.devices
        ));
    }
    out
}

fn proc_gpu() -> String {
    use crate::drivers::video::{self, gpu, sysfs};
    let (w, h) = video::resolution().unwrap_or((0, 0));
    let display = crate::drivers::pci::devices().into_iter().find(|d| d.class == 0x03);
    let identity = gpu::identity();
    let (codename, generation) = match &identity {
        Some((_, codename, generation)) => (codename.clone(), generation.clone()),
        None => (alloc::string::String::from("unknown"), alloc::string::String::from("unknown")),
    };
    let driver = if gpu::active() { format!("{} (display module)", gpu::name()) } else { alloc::string::String::from("boot framebuffer (no display module)") };
    let mut out = format!(
        "chipset\t{}\ncodename\t{}\ndriver\t{}\nframebuffer\t{}x{}\nstatus\t{}\n",
        sysfs::display_name(),
        codename,
        driver,
        w,
        h,
        if video::framebuffer_ready() { "ready" } else { "no linear framebuffer" }
    );
    if let Some(device) = display {
        out.push_str(&format!("pciid\t{:04x}:{:04x}\n", device.vendor, device.device));
        out.push_str(&format!("vendor\t{}\n", sysfs::vendor_name(device.vendor)));
        out.push_str(&format!("slot\t{}\n", sysfs::slot(&device)));
        out.push_str(&format!("generation\t{}\n", generation));
    }
    out.push_str(&gpu::describe());
    out
}

fn proc_mouse() -> String {
    use crate::drivers::input::mouse;
    let state = mouse::state();
    format!(
        "present\t{}\nposition\t{} {}\nbuttons\t{:03b}\nwheel\t{}\n",
        if mouse::present() { "yes" } else { "no" },
        state.x,
        state.y,
        state.buttons & 7,
        state.wheel
    )
}

fn proc_cmdline() -> String {
    format!("{}\n", crate::memory::cmdline())
}

fn proc_stat() -> String {
    crate::syscall::cpu_stats_text()
}

fn proc_hxinit() -> String {
    crate::hxinit::status_text()
}
