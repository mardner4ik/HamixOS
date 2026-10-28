use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    File,
    Dir,
    Symlink,
    CharDevice,
    BlockDevice,
    Fifo,
    Socket,
    Unknown,
}

impl FileKind {
    fn from_mode(mode: u32) -> FileKind {
        match mode & 0o170000 {
            0o100000 => FileKind::File,
            0o040000 => FileKind::Dir,
            0o120000 => FileKind::Symlink,
            0o020000 => FileKind::CharDevice,
            0o060000 => FileKind::BlockDevice,
            0o010000 => FileKind::Fifo,
            0o140000 => FileKind::Socket,
            _ => FileKind::Unknown,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            FileKind::File => "regular file",
            FileKind::Dir => "directory",
            FileKind::Symlink => "symbolic link",
            FileKind::CharDevice => "character device",
            FileKind::BlockDevice => "block device",
            FileKind::Fifo => "named pipe (FIFO)",
            FileKind::Socket => "socket",
            FileKind::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Metadata {
    pub kind: FileKind,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub size: u64,
    pub blocks: u64,
    pub block_size: u64,
    pub inode: u64,
    pub links: u64,
    pub device: u64,
    pub rdev: u64,
    pub accessed: i64,
    pub modified: i64,
    pub changed: i64,
}

#[derive(Clone, Copy, Debug)]
pub struct FsStats {
    pub block_size: u64,
    pub blocks: u64,
    pub free: u64,
    pub available: u64,
    pub files: u64,
    pub files_free: u64,
    pub read_only: bool,
    pub name_max: u64,
}

impl FsStats {
    pub fn total_bytes(&self) -> u64 {
        self.blocks.saturating_mul(self.block_size)
    }

    pub fn available_bytes(&self) -> u64 {
        self.available.saturating_mul(self.block_size)
    }

    pub fn used_bytes(&self) -> u64 {
        self.blocks.saturating_sub(self.free).saturating_mul(self.block_size)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct LocalTime {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    pub weekday: u32,
    pub offset: i64,
}

#[cfg(target_os = "hamix")]
mod sys {
    use super::{FileKind, FsStats, LocalTime, Metadata};
    use alloc::string::String;
    use alloc::vec::Vec;
    use core::ffi::c_char;
    use hxlibc_sys::libc;

    fn cstr(path: &str) -> Vec<u8> {
        let mut v = Vec::with_capacity(path.len() + 1);
        v.extend_from_slice(path.as_bytes());
        v.push(0);
        v
    }

    fn convert(st: &libc::stat) -> Metadata {
        Metadata {
            kind: FileKind::from_mode(st.st_mode),
            mode: st.st_mode,
            uid: st.st_uid,
            gid: st.st_gid,
            size: st.st_size.max(0) as u64,
            blocks: st.st_blocks.max(0) as u64,
            block_size: st.st_blksize.max(0) as u64,
            inode: st.st_ino,
            links: st.st_nlink as u64,
            device: st.st_dev,
            rdev: st.st_rdev,
            accessed: st.st_atime.tv_sec,
            modified: st.st_mtime.tv_sec,
            changed: st.st_ctime.tv_sec,
        }
    }

    pub fn metadata(path: &str) -> Option<Metadata> {
        let c = cstr(path);
        let mut st = libc::stat::zeroed();
        (unsafe { libc::stat(c.as_ptr() as *const c_char, &mut st) } == 0).then(|| convert(&st))
    }

    pub fn symlink_metadata(path: &str) -> Option<Metadata> {
        let c = cstr(path);
        let mut st = libc::stat::zeroed();
        (unsafe { libc::lstat(c.as_ptr() as *const c_char, &mut st) } == 0).then(|| convert(&st))
    }

    pub fn read_link(path: &str) -> Option<String> {
        let c = cstr(path);
        let mut buf = alloc::vec![0u8; 4096];
        let n = unsafe { libc::readlink(c.as_ptr() as *const c_char, buf.as_mut_ptr() as *mut c_char, buf.len()) };
        if n < 0 {
            return None;
        }
        buf.truncate(n as usize);
        Some(String::from_utf8_lossy(&buf).into_owned())
    }

    pub fn fs_stats(path: &str) -> Option<FsStats> {
        let c = cstr(path);
        let mut sv = libc::statvfs::zeroed();
        if unsafe { libc::statvfs(c.as_ptr() as *const c_char, &mut sv) } != 0 {
            return None;
        }
        let block_size = if sv.f_frsize != 0 { sv.f_frsize } else { sv.f_bsize };
        Some(FsStats { block_size, blocks: sv.f_blocks, free: sv.f_bfree, available: sv.f_bavail, files: sv.f_files, files_free: sv.f_ffree, read_only: sv.f_flag & 1 != 0, name_max: sv.f_namemax })
    }

    pub fn read_head(path: &str, limit: usize) -> Option<Vec<u8>> {
        let c = cstr(path);
        let fd = unsafe { libc::open(c.as_ptr() as *const c_char, 0) };
        if fd < 0 {
            return None;
        }
        let mut buf = alloc::vec![0u8; limit];
        let mut filled = 0;
        while filled < limit {
            let n = unsafe { libc::read(fd, buf[filled..].as_mut_ptr() as *mut core::ffi::c_void, limit - filled) };
            if n <= 0 {
                break;
            }
            filled += n as usize;
        }
        unsafe { libc::close(fd) };
        buf.truncate(filled);
        Some(buf)
    }

    pub fn local_time(seconds: i64) -> Option<LocalTime> {
        let mut tm = libc::tm::zeroed();
        if unsafe { libc::localtime_r(&seconds, &mut tm) }.is_null() {
            return None;
        }
        Some(LocalTime {
            year: tm.tm_year + 1900,
            month: (tm.tm_mon + 1) as u32,
            day: tm.tm_mday as u32,
            hour: tm.tm_hour as u32,
            minute: tm.tm_min as u32,
            second: tm.tm_sec as u32,
            weekday: tm.tm_wday as u32,
            offset: tm.tm_gmtoff as i64,
        })
    }
}

#[cfg(not(target_os = "hamix"))]
mod sys {
    use super::{FileKind, FsStats, LocalTime, Metadata};
    use std::os::unix::fs::MetadataExt;

    fn convert(m: &std::fs::Metadata) -> Metadata {
        Metadata {
            kind: FileKind::from_mode(m.mode()),
            mode: m.mode(),
            uid: m.uid(),
            gid: m.gid(),
            size: m.size(),
            blocks: m.blocks(),
            block_size: m.blksize(),
            inode: m.ino(),
            links: m.nlink(),
            device: m.dev(),
            rdev: m.rdev(),
            accessed: m.atime(),
            modified: m.mtime(),
            changed: m.ctime(),
        }
    }

    pub fn metadata(path: &str) -> Option<Metadata> {
        std::fs::metadata(path).ok().map(|m| convert(&m))
    }

    pub fn symlink_metadata(path: &str) -> Option<Metadata> {
        std::fs::symlink_metadata(path).ok().map(|m| convert(&m))
    }

    pub fn read_head(path: &str, limit: usize) -> Option<Vec<u8>> {
        use std::io::Read;
        let mut out = Vec::new();
        std::fs::File::open(path).ok()?.take(limit as u64).read_to_end(&mut out).ok()?;
        Some(out)
    }

    pub fn read_link(path: &str) -> Option<String> {
        std::fs::read_link(path).ok().map(|p| p.to_string_lossy().into_owned())
    }

    #[cfg(target_os = "linux")]
    #[repr(C)]
    struct Statvfs {
        bsize: u64,
        frsize: u64,
        blocks: u64,
        bfree: u64,
        bavail: u64,
        files: u64,
        ffree: u64,
        favail: u64,
        fsid: u64,
        flag: u64,
        namemax: u64,
        spare: [u64; 6],
    }

    #[cfg(target_os = "linux")]
    #[repr(C)]
    struct Tm {
        sec: i32,
        min: i32,
        hour: i32,
        mday: i32,
        mon: i32,
        year: i32,
        wday: i32,
        yday: i32,
        isdst: i32,
        gmtoff: i64,
        zone: *const u8,
    }

    #[cfg(target_os = "linux")]
    extern "C" {
        fn statvfs(path: *const u8, buf: *mut Statvfs) -> i32;
        fn localtime_r(time: *const i64, out: *mut Tm) -> *mut Tm;
    }

    #[cfg(target_os = "linux")]
    pub fn fs_stats(path: &str) -> Option<FsStats> {
        let c = std::ffi::CString::new(path).ok()?;
        let mut sv: Statvfs = unsafe { std::mem::zeroed() };
        if unsafe { statvfs(c.as_ptr() as *const u8, &mut sv) } != 0 {
            return None;
        }
        let block_size = if sv.frsize != 0 { sv.frsize } else { sv.bsize };
        Some(FsStats { block_size, blocks: sv.blocks, free: sv.bfree, available: sv.bavail, files: sv.files, files_free: sv.ffree, read_only: sv.flag & 1 != 0, name_max: sv.namemax })
    }

    #[cfg(not(target_os = "linux"))]
    pub fn fs_stats(_path: &str) -> Option<FsStats> {
        None
    }

    #[cfg(target_os = "linux")]
    pub fn local_time(seconds: i64) -> Option<LocalTime> {
        let mut tm: Tm = unsafe { std::mem::zeroed() };
        if unsafe { localtime_r(&seconds, &mut tm) }.is_null() {
            return None;
        }
        Some(LocalTime { year: tm.year + 1900, month: (tm.mon + 1) as u32, day: tm.mday as u32, hour: tm.hour as u32, minute: tm.min as u32, second: tm.sec as u32, weekday: tm.wday as u32, offset: tm.gmtoff })
    }

    #[cfg(not(target_os = "linux"))]
    pub fn local_time(seconds: i64) -> Option<LocalTime> {
        let days = seconds.div_euclid(86400);
        let rest = seconds.rem_euclid(86400);
        let z = days + 719468;
        let era = z.div_euclid(146097);
        let doe = z - era * 146097;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        let year = (yoe + era * 400 + if month <= 2 { 1 } else { 0 }) as i32;
        Some(LocalTime { year, month, day, hour: (rest / 3600) as u32, minute: (rest / 60 % 60) as u32, second: (rest % 60) as u32, weekday: (days + 4).rem_euclid(7) as u32, offset: 0 })
    }
}

pub use sys::{fs_stats, local_time, metadata, read_head, read_link, symlink_metadata};

pub fn format_time(seconds: i64) -> String {
    const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    match local_time(seconds) {
        Some(t) => {
            let sign = if t.offset < 0 { '-' } else { '+' };
            let off = t.offset.abs();
            format!("{}, {:04}-{:02}-{:02} {:02}:{:02}:{:02} {}{:02}{:02}", DAYS[(t.weekday % 7) as usize], t.year, t.month, t.day, t.hour, t.minute, t.second, sign, off / 3600, off / 60 % 60)
        }
        None => format!("{} s since 1970", seconds),
    }
}

pub fn permission_string(mode: u32) -> String {
    let kind = match FileKind::from_mode(mode) {
        FileKind::Dir => 'd',
        FileKind::Symlink => 'l',
        FileKind::CharDevice => 'c',
        FileKind::BlockDevice => 'b',
        FileKind::Fifo => 'p',
        FileKind::Socket => 's',
        _ => '-',
    };
    let mut out = String::new();
    out.push(kind);
    let bit = |m: u32, c: char| if mode & m != 0 { c } else { '-' };
    let exec = |x: u32, special: u32, set: char, unset: char| match (mode & x != 0, mode & special != 0) {
        (true, true) => set,
        (false, true) => unset,
        (true, false) => 'x',
        (false, false) => '-',
    };
    out.push(bit(0o400, 'r'));
    out.push(bit(0o200, 'w'));
    out.push(exec(0o100, 0o4000, 's', 'S'));
    out.push(bit(0o040, 'r'));
    out.push(bit(0o020, 'w'));
    out.push(exec(0o010, 0o2000, 's', 'S'));
    out.push(bit(0o004, 'r'));
    out.push(bit(0o002, 'w'));
    out.push(exec(0o001, 0o1000, 't', 'T'));
    out
}

pub fn group_names() -> Vec<(u32, String)> {
    crate::read_to_string("/etc/group")
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split(':').collect();
            Some((f.get(2)?.parse().ok()?, String::from(*f.first()?)))
        })
        .collect()
}

pub fn current_exe() -> Option<String> {
    read_link("/proc/self/exe").filter(|p| p.starts_with('/'))
}

pub struct Mount {
    pub device: String,
    pub point: String,
    pub fs_type: String,
    pub options: String,
}

pub fn mount_of(path: &str) -> Option<Mount> {
    let text = crate::read_to_string("/proc/mounts")?;
    let mut best: Option<Mount> = None;
    for line in text.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 4 {
            continue;
        }
        let point = f[1].replace("\\040", " ");
        let inside = point == "/" || path == point || path.starts_with(&format!("{}/", point.trim_end_matches('/')));
        if inside && best.as_ref().map(|b| point.len() >= b.point.len()).unwrap_or(true) {
            best = Some(Mount { device: String::from(f[0]), point, fs_type: String::from(f[2]), options: String::from(f[3]) });
        }
    }
    best
}

#[cfg(target_os = "hamix")]
pub fn can_access(path: &str, mode: i32) -> bool {
    let mut c = Vec::with_capacity(path.len() + 1);
    c.extend_from_slice(path.as_bytes());
    c.push(0);
    unsafe { hxlibc_sys::libc::access(c.as_ptr() as *const core::ffi::c_char, mode) == 0 }
}

#[cfg(not(target_os = "hamix"))]
pub fn can_access(path: &str, mode: i32) -> bool {
    extern "C" {
        fn access(path: *const std::ffi::c_char, mode: i32) -> i32;
    }
    let Ok(c) = std::ffi::CString::new(path) else {
        return false;
    };
    unsafe { access(c.as_ptr(), mode) == 0 }
}
