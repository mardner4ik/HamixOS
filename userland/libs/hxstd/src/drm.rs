use alloc::vec::Vec;

use crate::sys::{self, O_CLOEXEC, O_RDWR};

fn ioc(dir: u64, nr: u64, size: u64) -> u64 {
    (dir << 30) | (size << 16) | (0x64 << 8) | nr
}

const GETRESOURCES: (u64, u64) = (0xA0, 64);
const GETCRTC: (u64, u64) = (0xA1, 104);
const SETCRTC: (u64, u64) = (0xA2, 104);
const GETENCODER: (u64, u64) = (0xA6, 20);
const GETCONNECTOR: (u64, u64) = (0xA7, 80);
const ADDFB: (u64, u64) = (0xAE, 28);
const RMFB: (u64, u64) = (0xAF, 4);
const DIRTYFB: (u64, u64) = (0xB1, 24);
const CREATE_DUMB: (u64, u64) = (0xB2, 32);
const MAP_DUMB: (u64, u64) = (0xB3, 16);
const DESTROY_DUMB: (u64, u64) = (0xB4, 4);

fn call(fd: u64, (nr, size): (u64, u64), data: &mut [u8]) -> i64 {
    sys::ioctl(fd, ioc(3, nr, size), data.as_mut_ptr())
}

fn r32(s: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(s[at..at + 4].try_into().unwrap())
}

fn w32(s: &mut [u8], at: usize, v: u32) {
    s[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

fn w64(s: &mut [u8], at: usize, v: u64) {
    s[at..at + 8].copy_from_slice(&v.to_le_bytes());
}

fn ids(fd: u64, request: (u64, u64), data: &mut [u8], ptr_at: usize, count_at: usize) -> Vec<u32> {
    let count = r32(data, count_at) as usize;
    let mut list = alloc::vec![0u32; count];
    w64(data, ptr_at, list.as_mut_ptr() as u64);
    if call(fd, request, data) < 0 {
        return Vec::new();
    }
    list.truncate(r32(data, count_at) as usize);
    list
}

pub struct Drm {
    fd: u64,
    pub width: u32,
    pub height: u32,
    pub pitch: u32,
    pub map: *mut u8,
    size: u64,
    handle: u32,
    fb: u32,
    crtc: u32,
    connector: u32,
}

impl Drm {
    pub fn open() -> Option<Drm> {
        let fd = sys::open_with("/dev/dri/card0", O_RDWR | O_CLOEXEC);
        if fd < 0 {
            return None;
        }
        let fd = fd as u64;
        match Self::setup(fd) {
            Some(d) => Some(d),
            None => {
                sys::close(fd);
                None
            }
        }
    }

    fn setup(fd: u64) -> Option<Drm> {
        let mut res = [0u8; 64];
        if call(fd, GETRESOURCES, &mut res) < 0 {
            return None;
        }
        let connectors = ids(fd, GETRESOURCES, &mut res, 16, 40);
        let crtcs: Vec<u32> = {
            let mut again = [0u8; 64];
            call(fd, GETRESOURCES, &mut again);
            ids(fd, GETRESOURCES, &mut again, 8, 36)
        };
        for connector in connectors {
            let mut conn = [0u8; 80];
            w32(&mut conn, 48, connector);
            if call(fd, GETCONNECTOR, &mut conn) < 0 || r32(&conn, 60) != 1 || r32(&conn, 32) == 0 {
                continue;
            }
            let count = r32(&conn, 32) as usize;
            let mut modes = alloc::vec![0u8; count * 68];
            let mut again = [0u8; 80];
            w32(&mut again, 48, connector);
            w32(&mut again, 32, count as u32);
            w64(&mut again, 8, modes.as_mut_ptr() as u64);
            if call(fd, GETCONNECTOR, &mut again) < 0 {
                continue;
            }
            let mode: [u8; 68] = modes[0..68].try_into().unwrap();
            let crtc = {
                let encoder = r32(&again, 44);
                let mut enc = [0u8; 20];
                w32(&mut enc, 0, encoder);
                if encoder != 0 && call(fd, GETENCODER, &mut enc) == 0 && r32(&enc, 8) != 0 {
                    r32(&enc, 8)
                } else {
                    *crtcs.first()?
                }
            };
            let width = u16::from_le_bytes([mode[4], mode[5]]) as u32;
            let height = u16::from_le_bytes([mode[14], mode[15]]) as u32;
            let mut dumb = [0u8; 32];
            w32(&mut dumb, 0, height);
            w32(&mut dumb, 4, width);
            w32(&mut dumb, 8, 32);
            if call(fd, CREATE_DUMB, &mut dumb) < 0 {
                return None;
            }
            let handle = r32(&dumb, 16);
            let pitch = r32(&dumb, 20);
            let size = u64::from_le_bytes(dumb[24..32].try_into().unwrap());
            let mut add = [0u8; 28];
            w32(&mut add, 4, width);
            w32(&mut add, 8, height);
            w32(&mut add, 12, pitch);
            w32(&mut add, 16, 32);
            w32(&mut add, 20, 24);
            w32(&mut add, 24, handle);
            if call(fd, ADDFB, &mut add) < 0 {
                return None;
            }
            let fb = r32(&add, 0);
            let mut map = [0u8; 16];
            w32(&mut map, 0, handle);
            if call(fd, MAP_DUMB, &mut map) < 0 {
                return None;
            }
            let offset = u64::from_le_bytes(map[8..16].try_into().unwrap());
            let addr = sys::mmap_shared_fd(fd, size as usize, offset);
            if addr < 0 {
                return None;
            }
            let drm = Drm { fd, width, height, pitch, map: addr as *mut u8, size, handle, fb, crtc, connector };
            if !drm.show(&mode) {
                return None;
            }
            return Some(drm);
        }
        None
    }

    fn show(&self, mode: &[u8; 68]) -> bool {
        let mut set = [0u8; 104];
        let connectors = [self.connector];
        w64(&mut set, 0, connectors.as_ptr() as u64);
        w32(&mut set, 8, 1);
        w32(&mut set, 12, self.crtc);
        w32(&mut set, 16, self.fb);
        w32(&mut set, 32, 1);
        set[36..104].copy_from_slice(mode);
        call(self.fd, SETCRTC, &mut set) == 0
    }

    pub fn active(&self) -> bool {
        let mut crtc = [0u8; 104];
        w32(&mut crtc, 12, self.crtc);
        call(self.fd, GETCRTC, &mut crtc) == 0 && r32(&crtc, 16) == self.fb
    }

    pub fn dirty(&self, rects: &[(i32, i32, i32, i32)]) {
        let clips: Vec<u16> = rects
            .iter()
            .flat_map(|&(x, y, w, h)| {
                let x1 = x.clamp(0, self.width as i32) as u16;
                let y1 = y.clamp(0, self.height as i32) as u16;
                let x2 = (x + w).clamp(0, self.width as i32) as u16;
                let y2 = (y + h).clamp(0, self.height as i32) as u16;
                [x1, y1, x2, y2]
            })
            .collect();
        let mut cmd = [0u8; 24];
        w32(&mut cmd, 0, self.fb);
        w32(&mut cmd, 12, rects.len() as u32);
        w64(&mut cmd, 16, clips.as_ptr() as u64);
        call(self.fd, DIRTYFB, &mut cmd);
    }
}

impl Drop for Drm {
    fn drop(&mut self) {
        sys::munmap(self.map as u64, self.size);
        let mut fb = [0u8; 4];
        w32(&mut fb, 0, self.fb);
        call(self.fd, RMFB, &mut fb);
        let mut handle = [0u8; 4];
        w32(&mut handle, 0, self.handle);
        call(self.fd, DESTROY_DUMB, &mut handle);
        sys::close(self.fd);
    }
}
