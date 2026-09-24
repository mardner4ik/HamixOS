use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

use super::{Queue, Transport};
use crate::drivers::video::edid;
use crate::drivers::video::gpu::{self as display, ConnectorDesc, DisplayOps, FbDesc};
use crate::net::dma::DmaRegion;

const CMD_GET_DISPLAY_INFO: u32 = 0x0100;
const CMD_RESOURCE_CREATE_2D: u32 = 0x0101;
const CMD_RESOURCE_UNREF: u32 = 0x0102;
const CMD_SET_SCANOUT: u32 = 0x0103;
const CMD_RESOURCE_FLUSH: u32 = 0x0104;
const CMD_TRANSFER_TO_HOST_2D: u32 = 0x0105;
const CMD_RESOURCE_ATTACH_BACKING: u32 = 0x0106;
const CMD_GET_EDID: u32 = 0x010A;
const CMD_UPDATE_CURSOR: u32 = 0x0300;
const CMD_MOVE_CURSOR: u32 = 0x0301;

const RESP_OK_NODATA: u32 = 0x1100;
const RESP_OK_DISPLAY_INFO: u32 = 0x1101;
const RESP_OK_EDID: u32 = 0x1104;

const F_EDID: u64 = 1 << 1;
const EVENT_DISPLAY: u32 = 1;

const FORMAT_B8G8R8A8: u32 = 1;
const FORMAT_B8G8R8X8: u32 = 2;

const RESOURCE_CURSOR: u32 = 2;
const RESOURCES: [u32; 2] = [1, 3];
const RESOURCES_NEXT: [u32; 2] = [4, 5];

const HDR: usize = 24;
const CURSOR_SIDE: u32 = 64;
const DEFAULT_SIZE: (u32, u32) = (1024, 768);
const MAX_SCANOUTS: usize = 16;
const FRAME_US: u64 = 16_667;

const STANDARD: [(u32, u32); 10] = [(640, 480), (800, 600), (1024, 768), (1280, 720), (1280, 800), (1280, 1024), (1366, 768), (1440, 900), (1600, 900), (1920, 1080)];

#[derive(Clone, Copy, Default)]
struct Scanout {
    width: u32,
    height: u32,
    enabled: bool,
}

struct Gpu {
    transport: Box<dyn Transport>,
    control: Queue,
    cursor: Queue,
    request: DmaRegion,
    response: DmaRegion,
    cursor_request: DmaRegion,
    framebuffer: DmaRegion,
    cursor_image: Option<DmaRegion>,
    width: u32,
    height: u32,
    stride: u64,
    resources: [u32; 2],
    front: usize,
    edid: bool,
    scanouts: Vec<Scanout>,
    edids: Vec<Vec<u8>>,
    mirrors: Vec<(u32, u32, u32)>,
}

unsafe impl Send for Gpu {}

static GPU: Mutex<Option<Gpu>> = Mutex::new(None);

fn put32(buf: &mut [u8], at: usize, value: u32) {
    buf[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn put64(buf: &mut [u8], at: usize, value: u64) {
    buf[at..at + 8].copy_from_slice(&value.to_le_bytes());
}

fn get32(buf: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([buf[at], buf[at + 1], buf[at + 2], buf[at + 3]])
}

fn stride_of(width: u32, height: u32) -> u64 {
    (width as u64 * height as u64 * 4).div_ceil(4096) * 4096
}

impl Gpu {
    fn start(&mut self, kind: u32, words: &[u32]) -> usize {
        let buf = self.request.slice(0, 256);
        buf[..HDR].fill(0);
        put32(buf, 0, kind);
        for (i, word) in words.iter().enumerate() {
            put32(buf, HDR + i * 4, *word);
        }
        HDR + words.len() * 4
    }

    fn call(&mut self, request_len: usize, response_len: usize) -> Option<u32> {
        while self.control.pop().is_some() {}
        let parts = [(self.request.phys, request_len as u32, false), (self.response.phys, response_len as u32, true)];
        self.control.push(&parts)?;
        self.control.kick(self.transport.as_ref());
        self.control.wait(self.transport.as_ref(), 2000)?;
        Some(get32(self.response.slice(0, 4), 0))
    }

    fn simple(&mut self, len: usize) -> bool {
        self.call(len, HDR) == Some(RESP_OK_NODATA)
    }

    fn read_scanouts(&mut self) {
        let len = self.start(CMD_GET_DISPLAY_INFO, &[]);
        let mut scanouts = Vec::new();
        if self.call(len, HDR + MAX_SCANOUTS * 24) == Some(RESP_OK_DISPLAY_INFO) {
            let count = (self.transport.config32(8) as usize).clamp(1, MAX_SCANOUTS);
            let resp = self.response.slice(0, HDR + MAX_SCANOUTS * 24);
            for i in 0..count {
                let at = HDR + i * 24;
                scanouts.push(Scanout { width: get32(resp, at + 8), height: get32(resp, at + 12), enabled: get32(resp, at + 16) != 0 });
            }
        }
        if scanouts.is_empty() {
            scanouts.push(Scanout { width: DEFAULT_SIZE.0, height: DEFAULT_SIZE.1, enabled: true });
        }
        self.scanouts = scanouts;
        self.edids = (0..self.scanouts.len()).map(|i| self.read_edid(i as u32)).collect();
    }

    fn read_edid(&mut self, scanout: u32) -> Vec<u8> {
        if !self.edid {
            return Vec::new();
        }
        let len = self.start(CMD_GET_EDID, &[scanout, 0]);
        if self.call(len, HDR + 8 + 1024) != Some(RESP_OK_EDID) {
            return Vec::new();
        }
        let resp = self.response.slice(0, HDR + 8 + 1024);
        let size = (get32(resp, HDR) as usize).min(1024);
        resp[HDR + 8..HDR + 8 + size].to_vec()
    }

    fn preferred(&self) -> (u32, u32) {
        let first = self.scanouts.first().copied().unwrap_or_default();
        if first.enabled && (320..=4096).contains(&first.width) && (240..=4096).contains(&first.height) {
            return (first.width, first.height);
        }
        self.edids
            .first()
            .and_then(|raw| edid::parse(raw))
            .and_then(|e| e.preferred)
            .map(|t| (t.width, t.height))
            .filter(|(w, h)| *w <= 4096 && *h <= 4096)
            .unwrap_or(DEFAULT_SIZE)
    }

    fn create(&mut self, id: u32, format: u32, width: u32, height: u32) -> bool {
        let len = self.start(CMD_RESOURCE_CREATE_2D, &[id, format, width, height]);
        self.simple(len)
    }

    fn attach(&mut self, id: u32, phys: u64, bytes: u32) -> bool {
        let len = self.start(CMD_RESOURCE_ATTACH_BACKING, &[id, 1]);
        let buf = self.request.slice(0, 256);
        put64(buf, len, phys);
        put32(buf, len + 8, bytes);
        put32(buf, len + 12, 0);
        self.simple(len + 16)
    }

    fn scanout_to(&mut self, scanout: u32, id: u32, width: u32, height: u32) -> bool {
        let len = self.start(CMD_SET_SCANOUT, &[0, 0, width, height, scanout, id]);
        self.simple(len)
    }

    fn unref(&mut self, id: u32) -> bool {
        let len = self.start(CMD_RESOURCE_UNREF, &[id, 0]);
        self.simple(len)
    }

    fn transfer(&mut self, id: u32, x: u32, y: u32, w: u32, h: u32, stride: u32) -> bool {
        let len = self.start(CMD_TRANSFER_TO_HOST_2D, &[x, y, w, h]);
        let buf = self.request.slice(0, 256);
        put64(buf, len, y as u64 * stride as u64 + x as u64 * 4);
        put32(buf, len + 8, id);
        put32(buf, len + 12, 0);
        self.simple(len + 16)
    }

    fn flush_rect(&mut self, id: u32, x: u32, y: u32, w: u32, h: u32) -> bool {
        let len = self.start(CMD_RESOURCE_FLUSH, &[x, y, w, h, id, 0]);
        self.simple(len)
    }

    fn cursor_command(&mut self, kind: u32, x: u32, y: u32, resource: u32, hot_x: u32, hot_y: u32) -> bool {
        let buf = self.cursor_request.slice(0, 64);
        buf[..HDR + 32].fill(0);
        put32(buf, 0, kind);
        for (i, word) in [0, x, y, 0, resource, hot_x, hot_y, 0].iter().enumerate() {
            put32(buf, HDR + i * 4, *word);
        }
        while self.cursor.pop().is_some() {}
        if self.cursor.push(&[(self.cursor_request.phys, (HDR + 32) as u32, false)]).is_none() {
            return false;
        }
        self.cursor.kick(self.transport.as_ref());
        self.cursor.wait(self.transport.as_ref(), 2000).is_some()
    }

    fn build_buffers(&mut self, ids: [u32; 2], width: u32, height: u32) -> Option<DmaRegion> {
        let stride = stride_of(width, height);
        let region = DmaRegion::new(stride as usize * 2)?;
        let bytes = (width * height * 4) as u32;
        for (i, id) in ids.iter().enumerate() {
            if !self.create(*id, FORMAT_B8G8R8X8, width, height) || !self.attach(*id, region.phys + stride * i as u64, bytes) {
                for id in ids.iter() {
                    self.unref(*id);
                }
                return None;
            }
        }
        Some(region)
    }

    fn present(&mut self, index: usize) -> bool {
        let id = self.resources[index];
        let (w, h) = (self.width, self.height);
        if !self.transfer(id, 0, 0, w, h, w * 4) || !self.scanout_to(0, id, w, h) || !self.flush_rect(id, 0, 0, w, h) {
            return false;
        }
        for (scanout, mw, mh) in self.mirrors.clone() {
            self.scanout_to(scanout, id, mw.min(w), mh.min(h));
            self.flush_rect(id, 0, 0, mw.min(w), mh.min(h));
        }
        self.front = index;
        true
    }

    fn connector(&self, index: usize) -> ConnectorDesc {
        let scanout = self.scanouts.get(index).copied().unwrap_or_default();
        let parsed = self.edids.get(index).and_then(|raw| edid::parse(raw));
        let preferred = parsed.as_ref().and_then(|e| e.preferred);
        let mut desc = ConnectorDesc { kind: display::CONNECTOR_VIRTUAL, id: index as u32, ..ConnectorDesc::default() };
        desc.status = scanout.enabled as u32;
        desc.native_w = preferred.map(|p| p.width).unwrap_or(scanout.width);
        desc.native_h = preferred.map(|p| p.height).unwrap_or(scanout.height);
        desc.refresh = preferred.map(|p| p.refresh).unwrap_or(60);
        desc.edid_len = self.edids.get(index).map(|e| e.len() as u32).unwrap_or(0);
        desc.primary = (index == 0) as u32;
        let name = format!("Virtual-{}", index + 1);
        for (i, b) in name.bytes().take(15).enumerate() {
            desc.name[i] = b;
        }
        desc
    }

    fn modes(&self, index: usize) -> Vec<(u32, u32, u32)> {
        let mut out: Vec<(u32, u32, u32)> = Vec::new();
        if let Some(parsed) = self.edids.get(index).and_then(|raw| edid::parse(raw)) {
            for t in parsed.modes {
                if t.width <= 4096 && t.height <= 4096 && !out.iter().any(|m| m.0 == t.width && m.1 == t.height) {
                    out.push((t.width, t.height, t.refresh));
                }
            }
        }
        if out.is_empty() {
            let native = self.connector(index);
            out.push((native.native_w, native.native_h, 60));
            for (w, h) in STANDARD {
                if (w, h) != (native.native_w, native.native_h) {
                    out.push((w, h, 60));
                }
            }
        }
        out
    }
}

extern "C" fn flush(_: u64, x: u32, y: u32, w: u32, h: u32) -> i32 {
    let mut guard = GPU.lock();
    let Some(gpu) = guard.as_mut() else {
        return -19;
    };
    let (id, stride) = (gpu.resources[gpu.front], gpu.width * 4);
    if gpu.transfer(id, x, y, w, h, stride) && gpu.flush_rect(id, x, y, w, h) { 0 } else { -5 }
}

extern "C" fn set_mode(_: u64, width: u32, height: u32) -> i32 {
    if !(320..=4096).contains(&width) || !(240..=4096).contains(&height) {
        return -22;
    }
    let fb = {
        let mut guard = GPU.lock();
        let Some(gpu) = guard.as_mut() else {
            return -19;
        };
        if (width, height) == (gpu.width, gpu.height) {
            return 0;
        }
        let ids = if gpu.resources == RESOURCES { RESOURCES_NEXT } else { RESOURCES };
        let Some(region) = gpu.build_buffers(ids, width, height) else {
            return -5;
        };
        let old = gpu.resources;
        gpu.resources = ids;
        gpu.width = width;
        gpu.height = height;
        gpu.stride = stride_of(width, height);
        gpu.framebuffer = region;
        if !gpu.present(0) {
            return -5;
        }
        for id in old {
            gpu.unref(id);
        }
        crate::memory::FramebufferInfo { addr: gpu.framebuffer.phys, pitch: width * 4, width, height, bpp: 32 }
    };
    display::adopt(fb);
    0
}

extern "C" fn page_flip(_: u64, index: u32) -> i32 {
    let mut guard = GPU.lock();
    let Some(gpu) = guard.as_mut() else {
        return -19;
    };
    if index > 1 {
        return -22;
    }
    if gpu.present(index as usize) { 0 } else { -5 }
}

extern "C" fn wait_vblank(_: u64, timeout_ms: u32) -> i32 {
    let now = crate::task::uptime_ms() * 1000;
    let next = (now / FRAME_US + 1) * FRAME_US;
    let wait_ms = (next - now).div_ceil(1000).min(timeout_ms.max(1) as u64);
    crate::arch::delay_ms(wait_ms);
    0
}

extern "C" fn connectors(_: u64, out: *mut ConnectorDesc, max: u32) -> i32 {
    let guard = GPU.lock();
    let Some(gpu) = guard.as_ref() else {
        return -19;
    };
    if out.is_null() {
        return -22;
    }
    let count = gpu.scanouts.len().min(max as usize);
    for i in 0..count {
        unsafe { out.add(i).write(gpu.connector(i)) };
    }
    count as i32
}

extern "C" fn connector_modes(_: u64, id: u32, out: *mut u32, max: u32) -> i32 {
    let guard = GPU.lock();
    let Some(gpu) = guard.as_ref() else {
        return -19;
    };
    if out.is_null() || id as usize >= gpu.scanouts.len() {
        return -22;
    }
    let modes = gpu.modes(id as usize);
    let target = unsafe { core::slice::from_raw_parts_mut(out, max as usize * 3) };
    let count = modes.len().min(max as usize);
    for (i, (w, h, hz)) in modes.into_iter().take(count).enumerate() {
        target[i * 3] = w;
        target[i * 3 + 1] = h;
        target[i * 3 + 2] = hz;
    }
    count as i32
}

extern "C" fn read_edid(_: u64, id: u32, out: *mut u8, max: u32) -> i32 {
    let guard = GPU.lock();
    let Some(gpu) = guard.as_ref() else {
        return -19;
    };
    let Some(raw) = gpu.edids.get(id as usize) else {
        return -22;
    };
    if out.is_null() {
        return -22;
    }
    let n = raw.len().min(max as usize);
    unsafe { core::ptr::copy_nonoverlapping(raw.as_ptr(), out, n) };
    n as i32
}

extern "C" fn set_output(_: u64, id: u32, width: u32, height: u32) -> i32 {
    if id == 0 {
        return set_mode(0, width, height);
    }
    let mut guard = GPU.lock();
    let Some(gpu) = guard.as_mut() else {
        return -19;
    };
    if id as usize >= gpu.scanouts.len() {
        return -22;
    }
    gpu.mirrors.retain(|(s, _, _)| *s != id);
    if width == 0 || height == 0 {
        return if gpu.scanout_to(id, 0, 0, 0) { 0 } else { -5 };
    }
    let (w, h) = (width.min(gpu.width), height.min(gpu.height));
    let resource = gpu.resources[gpu.front];
    if !gpu.scanout_to(id, resource, w, h) || !gpu.flush_rect(resource, 0, 0, w, h) {
        return -5;
    }
    gpu.mirrors.push((id, w, h));
    0
}

extern "C" fn cursor_set(_: u64, pixels: *const u32, width: u32, height: u32, hot_x: u32, hot_y: u32) -> i32 {
    let mut guard = GPU.lock();
    let Some(gpu) = guard.as_mut() else {
        return -19;
    };
    let Some(image) = gpu.cursor_image.as_ref() else {
        return -95;
    };
    if pixels.is_null() || width > CURSOR_SIDE || height > CURSOR_SIDE {
        return -22;
    }
    let source = unsafe { core::slice::from_raw_parts(pixels, (width * height) as usize) };
    let target = image.slice(0, (CURSOR_SIDE * CURSOR_SIDE * 4) as usize);
    target.fill(0);
    for row in 0..height as usize {
        for column in 0..width as usize {
            let at = (row * CURSOR_SIDE as usize + column) * 4;
            target[at..at + 4].copy_from_slice(&source[row * width as usize + column].to_le_bytes());
        }
    }
    if gpu.transfer(RESOURCE_CURSOR, 0, 0, CURSOR_SIDE, CURSOR_SIDE, CURSOR_SIDE * 4) && gpu.cursor_command(CMD_UPDATE_CURSOR, 0, 0, RESOURCE_CURSOR, hot_x, hot_y) {
        0
    } else {
        -5
    }
}

extern "C" fn cursor_move(_: u64, x: i32, y: i32) -> i32 {
    let mut guard = GPU.lock();
    let Some(gpu) = guard.as_mut() else {
        return -19;
    };
    if gpu.cursor_image.is_none() {
        return -95;
    }
    if gpu.cursor_command(CMD_MOVE_CURSOR, x.max(0) as u32, y.max(0) as u32, RESOURCE_CURSOR, 0, 0) { 0 } else { -5 }
}

extern "C" fn cursor_hide(_: u64) -> i32 {
    let mut guard = GPU.lock();
    let Some(gpu) = guard.as_mut() else {
        return -19;
    };
    if gpu.cursor_image.is_none() {
        return -95;
    }
    if gpu.cursor_command(CMD_UPDATE_CURSOR, 0, 0, 0, 0, 0) { 0 } else { -5 }
}

pub fn poll_events() {
    let changed = {
        let Some(mut guard) = GPU.try_lock() else {
            return;
        };
        let Some(gpu) = guard.as_mut() else {
            return;
        };
        let events = gpu.transport.config32(0);
        if events & EVENT_DISPLAY == 0 {
            return;
        }
        gpu.transport.write_config32(4, events);
        gpu.read_scanouts();
        true
    };
    if changed {
        crate::drivers::klog::log("virtio-gpu: display configuration changed");
        display::hotplug();
    }
}

fn build(found: super::Found) -> Option<(Gpu, String)> {
    let mut transport = found.transport;
    let accepted = super::negotiate(transport.as_mut(), F_EDID)?;
    let control = Queue::new(transport.as_mut(), 0, 16)?;
    let cursor = Queue::new(transport.as_mut(), 1, 16)?;
    super::finish(transport.as_mut());
    let mut gpu = Gpu {
        transport,
        control,
        cursor,
        request: DmaRegion::new(4096)?,
        response: DmaRegion::new(8192)?,
        cursor_request: DmaRegion::new(4096)?,
        framebuffer: DmaRegion::new(4096)?,
        cursor_image: None,
        width: 0,
        height: 0,
        stride: 0,
        resources: RESOURCES,
        front: 0,
        edid: accepted & F_EDID != 0,
        scanouts: Vec::new(),
        edids: Vec::new(),
        mirrors: Vec::new(),
    };
    gpu.read_scanouts();
    let (width, height) = gpu.preferred();
    gpu.framebuffer = gpu.build_buffers(RESOURCES, width, height)?;
    gpu.width = width;
    gpu.height = height;
    gpu.stride = stride_of(width, height);
    if !gpu.present(0) {
        return None;
    }
    if let Some(image) = DmaRegion::new((CURSOR_SIDE * CURSOR_SIDE * 4) as usize) {
        let phys = image.phys;
        if gpu.create(RESOURCE_CURSOR, FORMAT_B8G8R8A8, CURSOR_SIDE, CURSOR_SIDE) && gpu.attach(RESOURCE_CURSOR, phys, CURSOR_SIDE * CURSOR_SIDE * 4) {
            gpu.cursor_image = Some(image);
        }
    }
    Some((gpu, found.location))
}

pub fn init() -> Option<String> {
    let found = super::take(super::ID_GPU).into_iter().next()?;
    let (gpu, location) = build(found)?;
    let (width, height, addr, cursor, outputs, has_edid) = (gpu.width, gpu.height, gpu.framebuffer.phys, gpu.cursor_image.is_some(), gpu.scanouts.len(), gpu.edid);
    *GPU.lock() = Some(gpu);
    let mut caps = display::CAP_FLUSH | display::CAP_MODESET | display::CAP_FLIP | display::CAP_VBLANK | display::CAP_CONNECTORS | display::CAP_HOTPLUG;
    if cursor {
        caps |= display::CAP_CURSOR;
    }
    let mut ops = DisplayOps::empty(caps);
    ops.flush = Some(flush);
    ops.set_mode = Some(set_mode);
    ops.connectors = Some(connectors);
    ops.connector_modes = Some(connector_modes);
    ops.edid = Some(read_edid);
    ops.set_output = Some(set_output);
    ops.page_flip = Some(page_flip);
    ops.wait_vblank = Some(wait_vblank);
    ops.buffers = 2;
    if cursor {
        ops.cursor_set = Some(cursor_set);
        ops.cursor_move = Some(cursor_move);
        ops.cursor_hide = Some(cursor_hide);
    }
    let fb = FbDesc { addr, pitch: width * 4, width, height, bpp: 32 };
    display::register("virtio-gpu", ops, fb).ok()?;
    Some(format!("virtio-gpu {}x{}, {} output(s){} ({})", width, height, outputs, if has_edid { ", EDID" } else { "" }, location))
}
