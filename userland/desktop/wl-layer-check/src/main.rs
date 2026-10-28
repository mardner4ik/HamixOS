#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use hxstd::unix;
use hxstd::{entry, env, println, sys};

struct Msg {
    buf: Vec<u8>,
}

impl Msg {
    fn new(object: u32, opcode: u16) -> Msg {
        let mut buf = Vec::new();
        buf.extend_from_slice(&object.to_le_bytes());
        buf.extend_from_slice(&(opcode as u32).to_le_bytes());
        Msg { buf }
    }

    fn u32(mut self, v: u32) -> Msg {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }

    fn string(mut self, s: &str) -> Msg {
        let len = s.len() + 1;
        self.buf.extend_from_slice(&(len as u32).to_le_bytes());
        self.buf.extend_from_slice(s.as_bytes());
        self.buf.push(0);
        while self.buf.len() % 4 != 0 {
            self.buf.push(0);
        }
        self
    }

    fn finish(mut self) -> Vec<u8> {
        let size = self.buf.len() as u32;
        let word = u32::from_le_bytes(self.buf[4..8].try_into().unwrap()) | (size << 16);
        self.buf[4..8].copy_from_slice(&word.to_le_bytes());
        self.buf
    }
}

struct Conn {
    fd: u64,
    inbuf: Vec<u8>,
}

impl Conn {
    fn send(&self, data: Vec<u8>, fds: &[u64]) {
        unix::send_with_fds(self.fd, &data, fds, 0);
    }

    fn next(&mut self, timeout_ms: u64) -> Option<(u32, u16, Vec<u8>)> {
        let deadline = sys::uptime_ms() + timeout_ms;
        loop {
            if self.inbuf.len() >= 8 {
                let size = (u32::from_le_bytes(self.inbuf[4..8].try_into().unwrap()) >> 16) as usize;
                if size >= 8 && self.inbuf.len() >= size {
                    let msg: Vec<u8> = self.inbuf.drain(..size).collect();
                    let object = u32::from_le_bytes(msg[0..4].try_into().unwrap());
                    let opcode = (u32::from_le_bytes(msg[4..8].try_into().unwrap()) & 0xFFFF) as u16;
                    return Some((object, opcode, msg[8..].to_vec()));
                }
            }
            if sys::uptime_ms() > deadline {
                return None;
            }
            let mut buf = [0u8; 4096];
            match unix::recv_with_fds(self.fd, &mut buf, unix::MSG_DONTWAIT) {
                Ok((0, _)) => return None,
                Ok((n, fds)) => {
                    for fd in fds {
                        sys::close(fd);
                    }
                    self.inbuf.extend_from_slice(&buf[..n]);
                }
                Err(_) => sys::sleep_ms(5),
            }
        }
    }
}

fn word(data: &[u8], index: usize) -> u32 {
    data.get(index * 4..index * 4 + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap())).unwrap_or(0)
}

fn read_string(data: &[u8], offset: usize) -> String {
    let len = word(data, offset / 4) as usize;
    let start = offset + 4;
    String::from_utf8_lossy(&data[start..(start + len).min(data.len())]).trim_end_matches('\0').into()
}

fn main() -> i32 {
    let dir = env::var("XDG_RUNTIME_DIR").map(String::from).unwrap_or_else(|| alloc::format!("/tmp/runtime-{}", sys::getuid()));
    let path = alloc::format!("{}/{}", dir, env::var("WAYLAND_DISPLAY").unwrap_or("wayland-0"));
    let mut fd = -1;
    for _ in 0..50 {
        fd = unix::connect_to(&path, unix::SOCK_STREAM);
        if fd >= 0 {
            break;
        }
        sys::sleep_ms(200);
    }
    if fd < 0 {
        println!("FAIL cannot connect to {}", path);
        return 1;
    }
    let mut c = Conn { fd: fd as u64, inbuf: Vec::new() };
    c.send(Msg::new(1, 1).u32(2).finish(), &[]);
    c.send(Msg::new(1, 0).u32(3).finish(), &[]);
    let mut globals: Vec<(u32, String, u32)> = Vec::new();
    while let Some((object, opcode, data)) = c.next(3000) {
        if object == 2 && opcode == 0 {
            globals.push((word(&data, 0), read_string(&data, 4), 0));
        }
        if object == 3 {
            break;
        }
    }
    let find = |name: &str| globals.iter().find(|g| g.1 == name).map(|g| g.0);
    let (Some(compositor), Some(shm), Some(layer_shell)) = (find("wl_compositor"), find("wl_shm"), find("zwlr_layer_shell_v1")) else {
        println!("FAIL zwlr_layer_shell_v1 is not advertised");
        return 1;
    };
    println!("PASS zwlr_layer_shell_v1 is advertised");
    c.send(Msg::new(2, 0).u32(compositor).string("wl_compositor").u32(4).u32(4).finish(), &[]);
    c.send(Msg::new(2, 0).u32(shm).string("wl_shm").u32(1).u32(5).finish(), &[]);
    c.send(Msg::new(2, 0).u32(layer_shell).string("zwlr_layer_shell_v1").u32(4).u32(6).finish(), &[]);
    c.send(Msg::new(4, 0).u32(7).finish(), &[]);
    c.send(Msg::new(6, 0).u32(8).u32(7).u32(0).u32(2).string("check-panel").finish(), &[]);
    c.send(Msg::new(8, 0).u32(0).u32(32).finish(), &[]);
    c.send(Msg::new(8, 1).u32(2 | 4 | 8).finish(), &[]);
    c.send(Msg::new(8, 2).u32(32).finish(), &[]);
    c.send(Msg::new(7, 6).finish(), &[]);
    let mut configure = None;
    while let Some((object, opcode, data)) = c.next(3000) {
        if object == 1 && opcode == 0 {
            println!("FAIL protocol error {} on object {}", word(&data, 1), word(&data, 0));
            return 1;
        }
        if object == 8 && opcode == 0 {
            configure = Some((word(&data, 0), word(&data, 1), word(&data, 2)));
            break;
        }
    }
    let Some((serial, width, height)) = configure else {
        println!("FAIL no configure for the layer surface");
        return 1;
    };
    println!("{} configure {}x{} stretches along the anchors", if width >= 640 && height == 32 { "PASS" } else { "FAIL" }, width, height);
    c.send(Msg::new(8, 6).u32(serial).finish(), &[]);
    let size = (width * height * 4) as u64;
    let mem = unix::memfd_create("layer-check");
    unix::ftruncate(mem as u64, size);
    if let Ok(ptr) = unix::map_shared(mem as u64, size, 0) {
        let pixels = unsafe { core::slice::from_raw_parts_mut(ptr as *mut u32, (width * height) as usize) };
        for p in pixels.iter_mut() {
            *p = 0xFF30_A050;
        }
    }
    c.send(Msg::new(5, 0).u32(9).u32(size as u32).finish(), &[mem as u64]);
    c.send(Msg::new(9, 0).u32(10).u32(0).u32(width).u32(height).u32(width * 4).u32(0).finish(), &[]);
    c.send(Msg::new(7, 1).u32(10).u32(0).u32(0).finish(), &[]);
    c.send(Msg::new(7, 2).u32(0).u32(0).u32(width).u32(height).finish(), &[]);
    c.send(Msg::new(7, 6).finish(), &[]);
    c.send(Msg::new(1, 0).u32(11).finish(), &[]);
    let mut done = false;
    while let Some((object, opcode, data)) = c.next(3000) {
        if object == 1 && opcode == 0 {
            println!("FAIL protocol error {} on object {}", word(&data, 1), word(&data, 0));
            return 1;
        }
        if object == 11 {
            done = true;
            break;
        }
    }
    println!("{} the layer buffer is committed without protocol errors", if done { "PASS" } else { "FAIL" });
    let mut ok = done;
    if env::args().iter().any(|a| a == "--toplevels") {
        let Some(manager) = find("zwlr_foreign_toplevel_manager_v1") else {
            println!("FAIL zwlr_foreign_toplevel_manager_v1 is not advertised");
            return 1;
        };
        c.send(Msg::new(2, 0).u32(manager).string("zwlr_foreign_toplevel_manager_v1").u32(3).u32(12).finish(), &[]);
        let mut handles: Vec<(u32, String)> = Vec::new();
        while let Some((object, opcode, data)) = c.next(3000) {
            if object == 12 && opcode == 0 {
                handles.push((word(&data, 0), String::new()));
            } else if opcode == 0 {
                if let Some(h) = handles.iter_mut().find(|(h, _)| *h == object) {
                    h.1 = read_string(&data, 0);
                }
            }
            if handles.iter().any(|(_, t)| t == "Calculator") {
                break;
            }
        }
        let calc = handles.iter().find(|(_, t)| t == "Calculator").map(|(h, _)| *h);
        println!("{} foreign toplevels list the Calculator window ({} toplevel(s))", if calc.is_some() { "PASS" } else { "FAIL" }, handles.len());
        if let Some(handle) = calc {
            c.send(Msg::new(handle, 5).finish(), &[]);
            let mut closed = false;
            while let Some((object, opcode, _)) = c.next(4000) {
                if object == handle && opcode == 6 {
                    closed = true;
                    break;
                }
            }
            println!("{} closing it through the handle ends the toplevel", if closed { "PASS" } else { "FAIL" });
            ok &= closed;
        } else {
            ok = false;
        }
    }
    if env::args().iter().any(|a| a == "--clipboard") {
        let Some(ddm) = find("wl_data_device_manager") else {
            println!("FAIL wl_data_device_manager is not advertised");
            return 1;
        };
        let Some(seat) = find("wl_seat") else {
            println!("FAIL wl_seat is not advertised");
            return 1;
        };
        c.send(Msg::new(2, 0).u32(ddm).string("wl_data_device_manager").u32(3).u32(20).finish(), &[]);
        c.send(Msg::new(2, 0).u32(seat).string("wl_seat").u32(7).u32(21).finish(), &[]);
        c.send(Msg::new(20, 0).u32(22).finish(), &[]);
        c.send(Msg::new(22, 0).string("text/plain;charset=utf-8").finish(), &[]);
        c.send(Msg::new(20, 1).u32(23).u32(21).finish(), &[]);
        c.send(Msg::new(23, 1).u32(22).u32(1).finish(), &[]);
        let mut served = false;
        let deadline = sys::uptime_ms() + 3000;
        while sys::uptime_ms() < deadline && !served {
            let mut buf = [0u8; 4096];
            if let Ok((n, fds)) = unix::recv_with_fds(c.fd, &mut buf, unix::MSG_DONTWAIT) {
                c.inbuf.extend_from_slice(&buf[..n]);
                for fd in fds {
                    sys::write(fd, b"copied from a Wayland client");
                    sys::close(fd);
                    served = true;
                }
            } else {
                sys::sleep_ms(5);
            }
        }
        println!("{} the bridge asks the Wayland source for its data", if served { "PASS" } else { "FAIL" });
        sys::sleep_ms(300);
        let got = hxclient::v2::Connection::open().ok().and_then(|mut conn| conn.selection(""));
        let ok_clip = got.as_ref().map(|(_, d)| d == "copied from a Wayland client").unwrap_or(false);
        println!("{} native clients see the Wayland selection through hxproto", if ok_clip { "PASS" } else { "FAIL" });
        ok &= served && ok_clip;
    }
    if env::args().iter().any(|a| a == "--protocols") {
        let (Some(viewporter), Some(fractional), Some(xdg_output), Some(output)) = (find("wp_viewporter"), find("wp_fractional_scale_manager_v1"), find("zxdg_output_manager_v1"), find("wl_output")) else {
            println!("FAIL wp_viewporter, wp_fractional_scale_manager_v1 or zxdg_output_manager_v1 is not advertised");
            return 1;
        };
        println!("PASS viewporter, fractional-scale and xdg-output are advertised");
        c.send(Msg::new(2, 0).u32(viewporter).string("wp_viewporter").u32(1).u32(30).finish(), &[]);
        c.send(Msg::new(2, 0).u32(fractional).string("wp_fractional_scale_manager_v1").u32(1).u32(31).finish(), &[]);
        c.send(Msg::new(2, 0).u32(output).string("wl_output").u32(4).u32(32).finish(), &[]);
        c.send(Msg::new(2, 0).u32(xdg_output).string("zxdg_output_manager_v1").u32(3).u32(33).finish(), &[]);
        c.send(Msg::new(31, 1).u32(34).u32(7).finish(), &[]);
        c.send(Msg::new(33, 1).u32(35).u32(32).finish(), &[]);
        c.send(Msg::new(30, 1).u32(36).u32(7).finish(), &[]);
        c.send(Msg::new(36, 1).u32(0).u32(0).u32((width / 2) << 8).u32((height / 2) << 8).finish(), &[]);
        c.send(Msg::new(36, 2).u32(width).u32(height).finish(), &[]);
        c.send(Msg::new(7, 1).u32(10).u32(0).u32(0).finish(), &[]);
        c.send(Msg::new(7, 6).finish(), &[]);
        c.send(Msg::new(1, 0).u32(37).finish(), &[]);
        let (mut scale, mut logical, mut output_done, mut synced, mut error) = (None, None, false, false, None);
        while let Some((object, opcode, data)) = c.next(3000) {
            match (object, opcode) {
                (1, 0) => error = Some((word(&data, 0), word(&data, 1))),
                (34, 0) => scale = Some(word(&data, 0)),
                (35, 1) => logical = Some((word(&data, 0), word(&data, 1))),
                (32, 2) => output_done = true,
                (37, _) => synced = true,
                _ => {}
            }
            if synced && output_done && error.is_none() {
                break;
            }
        }
        if let Some((obj, code)) = error {
            println!("FAIL protocol error {} on object {}", code, obj);
            return 1;
        }
        println!("{} fractional scale reports 1.0 ({:?})", if scale == Some(120) { "PASS" } else { "FAIL" }, scale);
        println!("{} xdg-output gives the logical size {:?} and ends with wl_output.done", if logical.map(|(w, h)| w > 0 && h > 0).unwrap_or(false) && output_done { "PASS" } else { "FAIL" }, logical);
        println!("{} a scaled viewport commits without protocol errors", if synced { "PASS" } else { "FAIL" });
        ok &= scale == Some(120) && logical.is_some() && output_done && synced;
    }
    let hold = env::args().iter().any(|a| a == "--hold");
    sys::sleep_ms(if hold { 30000 } else { 1500 });
    if ok { 0 } else { 1 }
}

entry!(main);
