const MAGIC: u32 = 0xd00d_feed;
const HEADER_LEN: usize = 40;
const MAX_BLOB: usize = 16 << 20;
const MAX_DEPTH: usize = 16;

const BEGIN_NODE: u32 = 1;
const END_NODE: u32 = 2;
const PROP: u32 = 3;
const NOP: u32 = 4;

fn be32(bytes: &[u8], offset: usize) -> Option<u32> {
    let b = bytes.get(offset..offset.checked_add(4)?)?;
    Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

fn be64(bytes: &[u8], offset: usize) -> Option<u64> {
    Some(((be32(bytes, offset)? as u64) << 32) | be32(bytes, offset + 4)? as u64)
}

fn cells(bytes: &[u8], offset: usize, count: u32) -> Option<u64> {
    match count {
        0 => Some(0),
        1 => be32(bytes, offset).map(|v| v as u64),
        2 => be64(bytes, offset),
        _ => be64(bytes, offset + (count as usize - 2) * 4),
    }
}

fn cstr(bytes: &[u8], offset: usize) -> Option<&str> {
    let rest = bytes.get(offset..)?;
    let len = rest.iter().position(|&b| b == 0)?;
    core::str::from_utf8(&rest[..len]).ok()
}

fn align4(value: usize) -> usize {
    (value + 3) & !3
}

fn strip_unit(name: &str) -> &str {
    name.split('@').next().unwrap_or(name)
}

static CURRENT: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

pub fn remember(addr: usize) {
    CURRENT.store(addr, core::sync::atomic::Ordering::Relaxed);
}

pub fn current() -> Option<Fdt<'static>> {
    let addr = CURRENT.load(core::sync::atomic::Ordering::Relaxed);
    if addr == 0 {
        return None;
    }
    unsafe { Fdt::from_addr(addr) }
}

#[derive(Clone, Copy)]
pub struct Fdt<'a> {
    blob: &'a [u8],
    structs: &'a [u8],
    strings: &'a [u8],
    rsvmap: usize,
}

impl Fdt<'static> {
    pub unsafe fn from_addr(addr: usize) -> Option<Self> {
        if addr == 0 || addr % 4 != 0 {
            return None;
        }
        let header = unsafe { core::slice::from_raw_parts(addr as *const u8, HEADER_LEN) };
        if be32(header, 0)? != MAGIC {
            return None;
        }
        let total = be32(header, 4)? as usize;
        if !(HEADER_LEN..=MAX_BLOB).contains(&total) {
            return None;
        }
        Fdt::new(unsafe { core::slice::from_raw_parts(addr as *const u8, total) })
    }
}

impl<'a> Fdt<'a> {
    pub fn new(blob: &'a [u8]) -> Option<Self> {
        if be32(blob, 0)? != MAGIC || be32(blob, 20)? < 16 {
            return None;
        }
        let structs_at = be32(blob, 8)? as usize;
        let strings_at = be32(blob, 12)? as usize;
        let rsvmap = be32(blob, 16)? as usize;
        let strings_len = be32(blob, 32)? as usize;
        let structs_len = be32(blob, 36)? as usize;
        Some(Self {
            blob,
            structs: blob.get(structs_at..structs_at.checked_add(structs_len)?)?,
            strings: blob.get(strings_at..strings_at.checked_add(strings_len)?)?,
            rsvmap,
        })
    }

    pub fn address(&self) -> usize {
        self.blob.as_ptr() as usize
    }

    pub fn total_size(&self) -> usize {
        self.blob.len()
    }

    pub fn reservations(&self) -> impl Iterator<Item = (u64, u64)> + 'a {
        let blob = self.blob;
        let mut offset = self.rsvmap;
        core::iter::from_fn(move || {
            let base = be64(blob, offset)?;
            let size = be64(blob, offset + 8)?;
            if base == 0 && size == 0 {
                return None;
            }
            offset += 16;
            Some((base, size))
        })
    }

    pub fn nodes(&self) -> Nodes<'a> {
        Nodes::new(*self, 0, 0, (2, 1))
    }

    pub fn root(&self) -> Option<Node<'a>> {
        self.nodes().next()
    }

    pub fn find(&self, path: &str) -> Option<Node<'a>> {
        let want = path.split('/').filter(|c| !c.is_empty()).count();
        let mut nodes = self.nodes();
        while let Some(node) = nodes.next() {
            if node.depth == want && nodes.path_matches(path) {
                return Some(node);
            }
        }
        None
    }

    pub fn resolve(&self, path_or_alias: &str) -> Option<Node<'a>> {
        if path_or_alias.starts_with('/') {
            return self.find(path_or_alias);
        }
        let target = self.find("/aliases")?.str_property(path_or_alias)?;
        self.find(target)
    }

    pub fn find_compatible(&self, compatible: &str) -> Option<Node<'a>> {
        self.nodes().find(|n| n.compatible_with(compatible) && n.enabled())
    }

    pub fn chosen(&self) -> Option<Node<'a>> {
        self.find("/chosen")
    }

    fn token(&self, offset: usize) -> Option<u32> {
        be32(self.structs, offset)
    }

    fn string(&self, offset: usize) -> Option<&'a str> {
        cstr(self.strings, offset)
    }
}

pub struct Nodes<'a> {
    fdt: Fdt<'a>,
    offset: usize,
    depth: usize,
    floor: usize,
    cells: [(u32, u32); MAX_DEPTH + 1],
    path: [&'a str; MAX_DEPTH],
    done: bool,
}

impl<'a> Nodes<'a> {
    fn new(fdt: Fdt<'a>, offset: usize, depth: usize, parent_cells: (u32, u32)) -> Self {
        let mut cells = [(2, 1); MAX_DEPTH + 1];
        cells[depth.min(MAX_DEPTH)] = parent_cells;
        Self { fdt, offset, depth, floor: depth, cells, path: [""; MAX_DEPTH], done: false }
    }

    fn path_matches(&self, path: &str) -> bool {
        let mut components = path.split('/').filter(|c| !c.is_empty());
        for level in 1..self.depth.min(MAX_DEPTH) {
            let Some(want) = components.next() else {
                return false;
            };
            let have = self.path[level];
            let same = if want.contains('@') { have == want } else { strip_unit(have) == want };
            if !same {
                return false;
            }
        }
        components.next().is_none()
    }
}

impl<'a> Iterator for Nodes<'a> {
    type Item = Node<'a>;

    fn next(&mut self) -> Option<Node<'a>> {
        while !self.done {
            let Some(token) = self.fdt.token(self.offset) else {
                self.done = true;
                break;
            };
            match token {
                BEGIN_NODE => {
                    let name_at = self.offset + 4;
                    let Some(name) = cstr(self.fdt.structs, name_at) else {
                        self.done = true;
                        break;
                    };
                    let props = align4(name_at + name.len() + 1);
                    let depth = self.depth;
                    let parent = self.cells[depth.min(MAX_DEPTH)];
                    let mut node = Node { fdt: self.fdt, name, depth, props, cells: parent, child_cells: (2, 1) };
                    node.child_cells = (
                        node.u32_property("#address-cells").unwrap_or(2),
                        node.u32_property("#size-cells").unwrap_or(1),
                    );
                    if depth < MAX_DEPTH {
                        self.path[depth] = name;
                        self.cells[depth + 1] = node.child_cells;
                    }
                    self.depth += 1;
                    self.offset = props;
                    return Some(node);
                }
                END_NODE => {
                    if self.depth <= self.floor {
                        self.done = true;
                        break;
                    }
                    self.depth -= 1;
                    self.offset += 4;
                }
                PROP => {
                    let Some(len) = self.fdt.token(self.offset + 4) else {
                        self.done = true;
                        break;
                    };
                    self.offset = align4(self.offset + 12 + len as usize);
                }
                NOP => self.offset += 4,
                _ => self.done = true,
            }
        }
        None
    }
}

#[derive(Clone, Copy)]
pub struct Node<'a> {
    fdt: Fdt<'a>,
    pub name: &'a str,
    pub depth: usize,
    props: usize,
    cells: (u32, u32),
    child_cells: (u32, u32),
}

impl<'a> Node<'a> {
    pub fn properties(&self) -> Properties<'a> {
        Properties { fdt: self.fdt, offset: self.props }
    }

    pub fn property(&self, name: &str) -> Option<&'a [u8]> {
        self.properties().find(|(n, _)| *n == name).map(|(_, v)| v)
    }

    pub fn str_property(&self, name: &str) -> Option<&'a str> {
        let value = self.property(name)?;
        let end = value.iter().position(|&b| b == 0).unwrap_or(value.len());
        core::str::from_utf8(&value[..end]).ok()
    }

    pub fn u32_property(&self, name: &str) -> Option<u32> {
        be32(self.property(name)?, 0)
    }

    pub fn u64_property(&self, name: &str) -> Option<u64> {
        let value = self.property(name)?;
        match value.len() {
            4 => be32(value, 0).map(|v| v as u64),
            _ => be64(value, 0),
        }
    }

    pub fn strings(&self, name: &str) -> impl Iterator<Item = &'a str> {
        self.property(name)
            .unwrap_or(&[])
            .split(|&b| b == 0)
            .filter(|s| !s.is_empty())
            .filter_map(|s| core::str::from_utf8(s).ok())
    }

    pub fn compatible_with(&self, what: &str) -> bool {
        self.strings("compatible").any(|c| c == what)
    }

    pub fn enabled(&self) -> bool {
        matches!(self.str_property("status"), None | Some("okay") | Some("ok"))
    }

    pub fn reg(&self) -> impl Iterator<Item = (u64, u64)> + use<'a> {
        let value = self.property("reg").unwrap_or(&[]);
        let (address_cells, size_cells) = self.cells;
        let stride = (address_cells + size_cells) as usize * 4;
        let count = if stride == 0 { 0 } else { value.len() / stride };
        (0..count).filter_map(move |i| {
            let at = i * stride;
            Some((cells(value, at, address_cells)?, cells(value, at + address_cells as usize * 4, size_cells)?))
        })
    }

    pub fn children(&self) -> impl Iterator<Item = Node<'a>> {
        let depth = self.depth + 1;
        Nodes::new(self.fdt, self.props, depth, self.child_cells).filter(move |n| n.depth == depth)
    }
}

pub struct Properties<'a> {
    fdt: Fdt<'a>,
    offset: usize,
}

impl<'a> Iterator for Properties<'a> {
    type Item = (&'a str, &'a [u8]);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.fdt.token(self.offset)? {
                NOP => self.offset += 4,
                PROP => {
                    let len = self.fdt.token(self.offset + 4)? as usize;
                    let name_offset = self.fdt.token(self.offset + 8)? as usize;
                    let start = self.offset + 12;
                    let value = self.fdt.structs.get(start..start + len)?;
                    self.offset = align4(start + len);
                    return Some((self.fdt.string(name_offset)?, value));
                }
                _ => return None,
            }
        }
    }
}
