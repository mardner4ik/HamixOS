#![no_std]

mod pci;
mod virtq;

use hamix_kpi::{self as kpi, ConnectorDesc, DisplayOps, FbDesc, PciHandle};

use pci::Transport;
use virtq::Ring;

const VENDOR: u16 = 0x1AF4;
const DEVICE: u16 = 0x1050;

const QUEUE_CONTROL: u16 = 0;
const QUEUE_CURSOR: u16 = 1;
const RING_SIZE: u16 = 16;

const CMD_GET_DISPLAY_INFO: u32 = 0x0100;
const CMD_RESOURCE_CREATE_2D: u32 = 0x0101;
const CMD_RESOURCE_UNREF: u32 = 0x0102;
const CMD_SET_SCANOUT: u32 = 0x0103;
const CMD_RESOURCE_FLUSH: u32 = 0x0104;
const CMD_TRANSFER_TO_HOST_2D: u32 = 0x0105;
const CMD_RESOURCE_ATTACH_BACKING: u32 = 0x0106;
const CMD_GET_EDID: u32 = 0x010A;
const RESP_OK_EDID: u32 = 0x1104;
const F_EDID: u32 = 1 << 1;
const EVENT_DISPLAY: u32 = 1;
const MAX_SCANOUTS: usize = 16;
const EDID_MAX: usize = 1024;
const FRAME_MS: u64 = 16;
const CMD_UPDATE_CURSOR: u32 = 0x0300;
const CMD_MOVE_CURSOR: u32 = 0x0301;

const RESP_OK_NODATA: u32 = 0x1100;
const RESP_OK_DISPLAY_INFO: u32 = 0x1101;

const FORMAT_B8G8R8X8: u32 = 2;
const FORMAT_B8G8R8A8: u32 = 1;

const RESOURCE_CURSOR: u32 = 2;
const RESOURCES: [u32; 2] = [1, 3];
const RESOURCES_NEXT: [u32; 2] = [4, 5];

const HDR_LEN: usize = 24;
const REQUEST_MAX: usize = 256;
const RESPONSE_MAX: usize = HDR_LEN + 8 + EDID_MAX;
const CURSOR_SIDE: u32 = 64;
const DEFAULT_WIDTH: u32 = 1024;
const DEFAULT_HEIGHT: u32 = 768;
const TIMEOUT_US: u64 = 2_000_000;

struct Buffer {
    virt: *mut u8,
    phys: u64,
    len: usize,
}

impl Buffer {
    fn new(len: usize) -> Option<Buffer> {
        let mut phys = 0u64;
        let virt = unsafe { kpi::hamix_dma_alloc(len, &mut phys) };
        if virt.is_null() {
            return None;
        }
        Some(Buffer { virt, phys, len })
    }

    fn bytes(&self) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self.virt, self.len) }
    }

    fn release(self) {
        if !self.virt.is_null() {
            unsafe { kpi::hamix_dma_free(self.virt, self.len) };
        }
    }
}

#[derive(Clone, Copy)]
struct Scanout {
    width: u32,
    height: u32,
    enabled: bool,
    edid_len: usize,
    edid: [u8; EDID_MAX],
}

struct Gpu {
    transport: Transport,
    control: Ring,
    cursor: Ring,
    request: Buffer,
    response: Buffer,
    cursor_request: Buffer,
    framebuffer: Buffer,
    cursor_resource: Buffer,
    width: u32,
    height: u32,
    stride: u64,
    resources: [u32; 2],
    front: usize,
    cursor_ready: bool,
    edid: bool,
    count: usize,
    scanouts: [Scanout; 4],
    mirrors: [(u32, u32); 4],
}

static mut GPU: Option<Gpu> = None;

fn gpu() -> Option<&'static mut Gpu> {
    unsafe { (&mut *(&raw mut GPU)).as_mut() }
}

fn put32(buf: &mut [u8], at: usize, value: u32) {
    buf[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn put64(buf: &mut [u8], at: usize, value: u64) {
    buf[at..at + 8].copy_from_slice(&value.to_le_bytes());
}

fn get32(buf: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([buf[at], buf[at + 1], buf[at + 2], buf[at + 3]])
}

fn header(buf: &mut [u8], kind: u32) {
    for byte in buf[..HDR_LEN].iter_mut() {
        *byte = 0;
    }
    put32(buf, 0, kind);
}

impl Gpu {
    fn control_call(&mut self, request_len: usize, response_len: usize) -> Option<u32> {
        self.control.drain();
        let parts = [(self.request.phys, request_len as u32, false), (self.response.phys, response_len as u32, true)];
        self.control.submit(&parts)?;
        self.control.wait(TIMEOUT_US)?;
        Some(get32(self.response.bytes(), 0))
    }

    fn simple(&mut self, request_len: usize) -> bool {
        self.control_call(request_len, HDR_LEN) == Some(RESP_OK_NODATA)
    }

    fn read_scanouts(&mut self) {
        header(self.request.bytes(), CMD_GET_DISPLAY_INFO);
        self.count = 0;
        if self.control_call(HDR_LEN, HDR_LEN + MAX_SCANOUTS * 24) == Some(RESP_OK_DISPLAY_INFO) {
            let wanted = (self.transport.device32(8) as usize).clamp(1, self.scanouts.len());
            for i in 0..wanted {
                let resp = self.response.bytes();
                let at = HDR_LEN + i * 24;
                self.scanouts[i].width = get32(resp, at + 8);
                self.scanouts[i].height = get32(resp, at + 12);
                self.scanouts[i].enabled = get32(resp, at + 16) != 0;
                self.scanouts[i].edid_len = 0;
            }
            self.count = wanted;
        }
        if self.count == 0 {
            self.count = 1;
            self.scanouts[0] = Scanout { width: DEFAULT_WIDTH, height: DEFAULT_HEIGHT, enabled: true, edid_len: 0, edid: [0; EDID_MAX] };
        }
        if self.edid {
            for i in 0..self.count {
                self.read_edid(i);
            }
        }
    }

    fn read_edid(&mut self, index: usize) {
        let buf = self.request.bytes();
        header(buf, CMD_GET_EDID);
        put32(buf, HDR_LEN, index as u32);
        put32(buf, HDR_LEN + 4, 0);
        if self.control_call(HDR_LEN + 8, RESPONSE_MAX) != Some(RESP_OK_EDID) {
            return;
        }
        let resp = self.response.bytes();
        let size = (get32(resp, HDR_LEN) as usize).min(EDID_MAX);
        self.scanouts[index].edid[..size].copy_from_slice(&resp[HDR_LEN + 8..HDR_LEN + 8 + size]);
        self.scanouts[index].edid_len = size;
    }

    fn edid_modes(&self, index: usize, out: &mut [u32]) -> usize {
        let scanout = &self.scanouts[index];
        if scanout.edid_len < 128 {
            return 0;
        }
        kpi::edid_modes(&scanout.edid[..scanout.edid_len], out)
    }

    fn preferred(&self) -> (u32, u32) {
        let first = self.scanouts[0];
        if first.enabled && first.width >= 320 && first.height >= 240 && first.width <= 4096 && first.height <= 4096 {
            return (first.width, first.height);
        }
        let mut modes = [0u32; 3];
        if self.edid_modes(0, &mut modes) > 0 && modes[0] <= 4096 && modes[1] <= 4096 {
            return (modes[0], modes[1]);
        }
        (DEFAULT_WIDTH, DEFAULT_HEIGHT)
    }

    fn create_resource(&mut self, id: u32, format: u32, width: u32, height: u32) -> bool {
        let buf = self.request.bytes();
        header(buf, CMD_RESOURCE_CREATE_2D);
        put32(buf, HDR_LEN, id);
        put32(buf, HDR_LEN + 4, format);
        put32(buf, HDR_LEN + 8, width);
        put32(buf, HDR_LEN + 12, height);
        self.simple(HDR_LEN + 16)
    }

    fn attach_backing(&mut self, id: u32, phys: u64, len: u32) -> bool {
        let buf = self.request.bytes();
        header(buf, CMD_RESOURCE_ATTACH_BACKING);
        put32(buf, HDR_LEN, id);
        put32(buf, HDR_LEN + 4, 1);
        put64(buf, HDR_LEN + 8, phys);
        put32(buf, HDR_LEN + 16, len);
        put32(buf, HDR_LEN + 20, 0);
        self.simple(HDR_LEN + 24)
    }

    fn set_scanout(&mut self, scanout: u32, id: u32, width: u32, height: u32) -> bool {
        let buf = self.request.bytes();
        header(buf, CMD_SET_SCANOUT);
        put32(buf, HDR_LEN, 0);
        put32(buf, HDR_LEN + 4, 0);
        put32(buf, HDR_LEN + 8, width);
        put32(buf, HDR_LEN + 12, height);
        put32(buf, HDR_LEN + 16, scanout);
        put32(buf, HDR_LEN + 20, id);
        self.simple(HDR_LEN + 24)
    }

    fn build_buffers(&mut self, ids: [u32; 2], width: u32, height: u32) -> Option<Buffer> {
        let stride = stride_of(width, height);
        let region = Buffer::new(stride as usize * 2)?;
        let bytes = width * height * 4;
        for (i, id) in ids.iter().enumerate() {
            if !self.create_resource(*id, FORMAT_B8G8R8X8, width, height) || !self.attach_backing(*id, region.phys + stride * i as u64, bytes) {
                for id in ids.iter() {
                    self.unref(*id);
                }
                region.release();
                return None;
            }
        }
        Some(region)
    }

    fn present(&mut self, index: usize) -> bool {
        let id = self.resources[index];
        let (w, h) = (self.width, self.height);
        if !self.transfer(id, 0, 0, w, h, w * 4) || !self.set_scanout(0, id, w, h) || !self.resource_flush(id, 0, 0, w, h) {
            return false;
        }
        for slot in 0..self.mirrors.len() {
            let (mw, mh) = self.mirrors[slot];
            if mw != 0 {
                self.set_scanout(slot as u32 + 1, id, mw.min(w), mh.min(h));
            }
        }
        self.front = index;
        true
    }

    fn connector(&self, index: usize) -> ConnectorDesc {
        let scanout = &self.scanouts[index];
        let mut name = [0u8; 16];
        name[..8].copy_from_slice(b"Virtual-");
        name[8] = b'1' + index as u8;
        let mut desc = ConnectorDesc { kind: kpi::CONNECTOR_VIRTUAL, id: index as u32, name, ..ConnectorDesc::default() };
        let mut modes = [0u32; 3];
        let (w, h, hz) = if self.edid_modes(index, &mut modes) > 0 { (modes[0], modes[1], modes[2]) } else { (scanout.width, scanout.height, 60) };
        desc.status = scanout.enabled as u32;
        desc.native_w = w;
        desc.native_h = h;
        desc.refresh = hz;
        desc.edid_len = scanout.edid_len as u32;
        desc.primary = (index == 0) as u32;
        desc
    }

    fn transfer(&mut self, id: u32, x: u32, y: u32, w: u32, h: u32, stride: u32) -> bool {
        let offset = y as u64 * stride as u64 + x as u64 * 4;
        let buf = self.request.bytes();
        header(buf, CMD_TRANSFER_TO_HOST_2D);
        put32(buf, HDR_LEN, x);
        put32(buf, HDR_LEN + 4, y);
        put32(buf, HDR_LEN + 8, w);
        put32(buf, HDR_LEN + 12, h);
        put64(buf, HDR_LEN + 16, offset);
        put32(buf, HDR_LEN + 24, id);
        put32(buf, HDR_LEN + 28, 0);
        self.simple(HDR_LEN + 32)
    }

    fn unref(&mut self, id: u32) -> bool {
        let buf = self.request.bytes();
        header(buf, CMD_RESOURCE_UNREF);
        put32(buf, HDR_LEN, id);
        put32(buf, HDR_LEN + 4, 0);
        self.simple(HDR_LEN + 8)
    }

    fn resource_flush(&mut self, id: u32, x: u32, y: u32, w: u32, h: u32) -> bool {
        let buf = self.request.bytes();
        header(buf, CMD_RESOURCE_FLUSH);
        put32(buf, HDR_LEN, x);
        put32(buf, HDR_LEN + 4, y);
        put32(buf, HDR_LEN + 8, w);
        put32(buf, HDR_LEN + 12, h);
        put32(buf, HDR_LEN + 16, id);
        put32(buf, HDR_LEN + 20, 0);
        self.simple(HDR_LEN + 24)
    }

    fn cursor_command(&mut self, kind: u32, x: u32, y: u32, resource: u32, hot_x: u32, hot_y: u32) -> bool {
        let buf = self.cursor_request.bytes();
        header(buf, kind);
        put32(buf, HDR_LEN, 0);
        put32(buf, HDR_LEN + 4, x);
        put32(buf, HDR_LEN + 8, y);
        put32(buf, HDR_LEN + 12, 0);
        put32(buf, HDR_LEN + 16, resource);
        put32(buf, HDR_LEN + 20, hot_x);
        put32(buf, HDR_LEN + 24, hot_y);
        put32(buf, HDR_LEN + 28, 0);
        self.cursor.drain();
        let parts = [(self.cursor_request.phys, (HDR_LEN + 32) as u32, false)];
        if self.cursor.submit(&parts).is_none() {
            return false;
        }
        self.cursor.wait(TIMEOUT_US).is_some()
    }
}

fn setup_queue(transport: &Transport, index: u16) -> Option<Ring> {
    transport.write16(pci::COMMON_QUEUE_SELECT, index);
    let available = transport.read16(pci::COMMON_QUEUE_SIZE);
    if available == 0 {
        return None;
    }
    let size = available.min(RING_SIZE);
    transport.write16(pci::COMMON_QUEUE_SIZE, size);
    let mut ring = Ring::new(index, size)?;
    transport.write64(pci::COMMON_QUEUE_DESC, ring.desc_phys);
    transport.write64(pci::COMMON_QUEUE_DRIVER, ring.avail_phys);
    transport.write64(pci::COMMON_QUEUE_DEVICE, ring.used_phys);
    let notify_off = transport.read16(pci::COMMON_QUEUE_NOTIFY_OFF);
    ring.set_notify(transport.notify_address(notify_off));
    transport.write16(pci::COMMON_QUEUE_ENABLE, 1);
    Some(ring)
}

fn reset(transport: &Transport) -> bool {
    transport.set_status(0);
    for _ in 0..1000 {
        if transport.status() == 0 {
            return true;
        }
        unsafe { kpi::hamix_udelay(100) };
    }
    false
}

fn negotiate(transport: &Transport) -> bool {
    transport.add_status(pci::STATUS_ACKNOWLEDGE);
    transport.add_status(pci::STATUS_DRIVER);
    if transport.device_features(1) & 1 == 0 {
        kpi::printk("virtio-gpu: device is not a virtio 1.0 device");
        return false;
    }
    transport.set_driver_features(0, transport.device_features(0) & F_EDID);
    transport.set_driver_features(1, 1);
    transport.add_status(pci::STATUS_FEATURES_OK);
    if transport.status() & pci::STATUS_FEATURES_OK == 0 {
        kpi::printk("virtio-gpu: device refused the feature set");
        return false;
    }
    true
}

fn stride_of(width: u32, height: u32) -> u64 {
    (width as u64 * height as u64 * 4).div_ceil(4096) * 4096
}

extern "C" fn flush(_context: u64, x: u32, y: u32, w: u32, h: u32) -> i32 {
    let Some(gpu) = gpu() else {
        return -19;
    };
    let stride = gpu.width * 4;
    let id = gpu.resources[gpu.front];
    if !gpu.transfer(id, x, y, w, h, stride) {
        return -5;
    }
    if !gpu.resource_flush(id, x, y, w, h) {
        return -5;
    }
    0
}

extern "C" fn set_mode(_context: u64, width: u32, height: u32) -> i32 {
    let Some(gpu) = gpu() else {
        return -19;
    };
    if width < 320 || height < 240 || width > 4096 || height > 4096 {
        return -22;
    }
    if width == gpu.width && height == gpu.height {
        return 0;
    }
    let ids = if gpu.resources == RESOURCES { RESOURCES_NEXT } else { RESOURCES };
    let Some(next) = gpu.build_buffers(ids, width, height) else {
        return -5;
    };
    let previous = core::mem::replace(&mut gpu.framebuffer, next);
    let old = gpu.resources;
    gpu.resources = ids;
    gpu.width = width;
    gpu.height = height;
    gpu.stride = stride_of(width, height);
    if !gpu.present(0) {
        return -5;
    }
    let fb = FbDesc { addr: gpu.framebuffer.virt as u64, pitch: width * 4, width, height, bpp: 32 };
    let adopted = unsafe { kpi::hamix_display_changed(&fb) } == 0;
    for id in old {
        gpu.unref(id);
    }
    previous.release();
    if adopted { 0 } else { -5 }
}

extern "C" fn page_flip(_context: u64, index: u32) -> i32 {
    let Some(gpu) = gpu() else {
        return -19;
    };
    if index > 1 {
        return -22;
    }
    if gpu.present(index as usize) { 0 } else { -5 }
}

extern "C" fn wait_vblank(_context: u64, timeout_ms: u32) -> i32 {
    let now = unsafe { kpi::hamix_uptime_ms() };
    let wait = (FRAME_MS - now % FRAME_MS).min(timeout_ms.max(1) as u64);
    unsafe { kpi::hamix_mdelay(wait) };
    0
}

extern "C" fn connectors(_context: u64, out: *mut ConnectorDesc, max: u32) -> i32 {
    let Some(gpu) = gpu() else {
        return -19;
    };
    if out.is_null() {
        return -22;
    }
    let count = gpu.count.min(max as usize);
    for i in 0..count {
        unsafe { out.add(i).write(gpu.connector(i)) };
    }
    count as i32
}

extern "C" fn connector_modes(_context: u64, id: u32, out: *mut u32, max: u32) -> i32 {
    let Some(gpu) = gpu() else {
        return -19;
    };
    if out.is_null() || id as usize >= gpu.count {
        return -22;
    }
    let target = unsafe { core::slice::from_raw_parts_mut(out, max as usize * 3) };
    let found = gpu.edid_modes(id as usize, target);
    if found > 0 {
        return found as i32;
    }
    if max == 0 {
        return 0;
    }
    let scanout = gpu.scanouts[id as usize];
    target[0] = if scanout.width >= 320 { scanout.width } else { DEFAULT_WIDTH };
    target[1] = if scanout.height >= 200 { scanout.height } else { DEFAULT_HEIGHT };
    target[2] = 60;
    1
}

extern "C" fn read_edid(_context: u64, id: u32, out: *mut u8, max: u32) -> i32 {
    let Some(gpu) = gpu() else {
        return -19;
    };
    if out.is_null() || id as usize >= gpu.count {
        return -22;
    }
    let scanout = &gpu.scanouts[id as usize];
    let n = scanout.edid_len.min(max as usize);
    unsafe { core::ptr::copy_nonoverlapping(scanout.edid.as_ptr(), out, n) };
    n as i32
}

extern "C" fn set_output(_context: u64, id: u32, width: u32, height: u32) -> i32 {
    if id == 0 {
        return set_mode(0, width, height);
    }
    let Some(gpu) = gpu() else {
        return -19;
    };
    let slot = id as usize - 1;
    if id as usize >= gpu.count || slot >= gpu.mirrors.len() {
        return -22;
    }
    if width == 0 || height == 0 {
        gpu.mirrors[slot] = (0, 0);
        return if gpu.set_scanout(id, 0, 0, 0) { 0 } else { -5 };
    }
    let (w, h) = (width.min(gpu.width), height.min(gpu.height));
    let resource = gpu.resources[gpu.front];
    if !gpu.set_scanout(id, resource, w, h) || !gpu.resource_flush(resource, 0, 0, w, h) {
        return -5;
    }
    gpu.mirrors[slot] = (w, h);
    0
}

extern "C" fn cursor_set(_context: u64, pixels: *const u32, width: u32, height: u32, hot_x: u32, hot_y: u32) -> i32 {
    let Some(gpu) = gpu() else {
        return -19;
    };
    if !gpu.cursor_ready || pixels.is_null() || width > CURSOR_SIDE || height > CURSOR_SIDE {
        return -22;
    }
    let source = unsafe { core::slice::from_raw_parts(pixels, (width * height) as usize) };
    let target = gpu.cursor_resource.bytes();
    for byte in target.iter_mut() {
        *byte = 0;
    }
    for row in 0..height as usize {
        let at = row * CURSOR_SIDE as usize * 4;
        for column in 0..width as usize {
            let value = source[row * width as usize + column];
            target[at + column * 4..at + column * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
    }
    if !gpu.transfer(RESOURCE_CURSOR, 0, 0, CURSOR_SIDE, CURSOR_SIDE, CURSOR_SIDE * 4) {
        return -5;
    }
    if !gpu.cursor_command(CMD_UPDATE_CURSOR, 0, 0, RESOURCE_CURSOR, hot_x, hot_y) {
        return -5;
    }
    0
}

extern "C" fn cursor_move(_context: u64, x: i32, y: i32) -> i32 {
    let Some(gpu) = gpu() else {
        return -19;
    };
    if !gpu.cursor_ready {
        return -95;
    }
    if gpu.cursor_command(CMD_MOVE_CURSOR, x.max(0) as u32, y.max(0) as u32, RESOURCE_CURSOR, 0, 0) { 0 } else { -5 }
}

extern "C" fn cursor_hide(_context: u64) -> i32 {
    let Some(gpu) = gpu() else {
        return -19;
    };
    if !gpu.cursor_ready {
        return -95;
    }
    if gpu.cursor_command(CMD_UPDATE_CURSOR, 0, 0, 0, 0, 0) { 0 } else { -5 }
}

fn build(handle: &PciHandle) -> Option<Gpu> {
    let transport = pci::discover(handle)?;
    if !reset(&transport) {
        kpi::printk("virtio-gpu: device did not come out of reset");
        return None;
    }
    if !negotiate(&transport) {
        return None;
    }
    if transport.num_queues() < 2 {
        kpi::printk("virtio-gpu: device has no cursor queue");
        return None;
    }
    let control = setup_queue(&transport, QUEUE_CONTROL)?;
    let cursor = setup_queue(&transport, QUEUE_CURSOR)?;
    transport.add_status(pci::STATUS_DRIVER_OK);

    let edid = transport.device_features(0) & F_EDID != 0;
    let request = Buffer::new(REQUEST_MAX)?;
    let response = Buffer::new(RESPONSE_MAX)?;
    let cursor_request = Buffer::new(REQUEST_MAX)?;
    let mut gpu = Gpu {
        transport,
        control,
        cursor,
        request,
        response,
        cursor_request,
        framebuffer: Buffer { virt: core::ptr::null_mut(), phys: 0, len: 0 },
        cursor_resource: Buffer { virt: core::ptr::null_mut(), phys: 0, len: 0 },
        width: 0,
        height: 0,
        stride: 0,
        resources: RESOURCES,
        front: 0,
        cursor_ready: false,
        edid,
        count: 0,
        scanouts: [Scanout { width: 0, height: 0, enabled: false, edid_len: 0, edid: [0; EDID_MAX] }; 4],
        mirrors: [(0, 0); 4],
    };
    gpu.read_scanouts();
    let (width, height) = gpu.preferred();
    let Some(framebuffer) = gpu.build_buffers(RESOURCES, width, height) else {
        kpi::printk("virtio-gpu: the device refused the scanout resources");
        return None;
    };
    gpu.framebuffer = framebuffer;
    gpu.width = width;
    gpu.height = height;
    gpu.stride = stride_of(width, height);
    if !gpu.present(0) {
        kpi::printk("virtio-gpu: the device refused to scan out the resource");
        return None;
    }
    if let Some(resource) = Buffer::new((CURSOR_SIDE * CURSOR_SIDE * 4) as usize) {
        gpu.cursor_resource = resource;
        let side = CURSOR_SIDE;
        let len = (side * side * 4) as usize;
        let phys = gpu.cursor_resource.phys;
        gpu.cursor_ready = gpu.create_resource(RESOURCE_CURSOR, FORMAT_B8G8R8A8, side, side) && gpu.attach_backing(RESOURCE_CURSOR, phys, len as u32);
    }
    Some(gpu)
}

fn init() -> i32 {
    if unsafe { kpi::hamix_display_abi() } != kpi::DISPLAY_ABI {
        kpi::printk("virtio-gpu: kernel display ABI does not match this module");
        return -1;
    }
    let Some(handle) = kpi::find_device(VENDOR, DEVICE) else {
        kpi::printk("virtio-gpu: no 1af4:1050 on this machine");
        return -1;
    };
    unsafe { kpi::hamix_pci_enable(&handle) };
    let Some(gpu) = build(&handle) else {
        return -1;
    };
    let (width, height, addr, cursor_ready) = (gpu.width, gpu.height, gpu.framebuffer.virt as u64, gpu.cursor_ready);
    unsafe { *(&raw mut GPU) = Some(gpu) };

    let mut caps = kpi::CAP_FLUSH | kpi::CAP_MODESET | kpi::CAP_FLIP | kpi::CAP_VBLANK | kpi::CAP_CONNECTORS | kpi::CAP_HOTPLUG;
    if cursor_ready {
        caps |= kpi::CAP_CURSOR;
    }
    let mut ops = DisplayOps::new(caps, 0);
    ops.flush = Some(flush);
    ops.set_mode = Some(set_mode);
    ops.connectors = Some(connectors);
    ops.connector_modes = Some(connector_modes);
    ops.edid = Some(read_edid);
    ops.set_output = Some(set_output);
    ops.page_flip = Some(page_flip);
    ops.wait_vblank = Some(wait_vblank);
    ops.buffers = 2;
    if cursor_ready {
        ops.cursor_set = Some(cursor_set);
        ops.cursor_move = Some(cursor_move);
        ops.cursor_hide = Some(cursor_hide);
    }
    let fb = FbDesc { addr, pitch: width * 4, width, height, bpp: 32 };
    if !kpi::register_display("virtio-gpu", &ops, &fb) {
        kpi::printk("virtio-gpu: the kernel refused the display registration");
        return -1;
    }
    if !kpi::claim(kpi::CLASS_DISPLAY, "virtio-gpu", &handle, None, core::ptr::null_mut()) {
        return -1;
    }
    if kpi::param("irq", 1) != 0 {
        match kpi::request_irq(&handle, interrupt, core::ptr::null_mut()) {
            Some(_) => kpi::dev_info("interrupts enabled"),
            None => kpi::dev_warn("no usable interrupt line, staying on the polled path"),
        }
    }
    kpi::printk("virtio-gpu: scanout ready");
    0
}

extern "C" fn interrupt(_context: *mut core::ffi::c_void) -> i32 {
    let Some(gpu) = gpu() else {
        return kpi::IRQ_NONE;
    };
    if !gpu.transport.isr.is_valid() {
        return kpi::IRQ_NONE;
    }
    let status = unsafe { kpi::hamix_readb(gpu.transport.isr.base) };
    if status & 2 != 0 {
        kpi::schedule_work(configuration_changed, core::ptr::null_mut());
    }
    if status == 0 { kpi::IRQ_NONE } else { kpi::IRQ_HANDLED }
}

extern "C" fn configuration_changed(_context: *mut core::ffi::c_void) {
    let Some(gpu) = gpu() else {
        return;
    };
    let events = gpu.transport.device32(0);
    if events & EVENT_DISPLAY == 0 {
        return;
    }
    gpu.transport.write_device32(4, events);
    gpu.read_scanouts();
    kpi::dev_info("display configuration changed");
    kpi::display_hotplug();
}

fn exit() {
    if let Some(gpu) = gpu() {
        gpu.control.release();
        gpu.cursor.release();
    }
}

fn suspend() {
    if let Some(gpu) = gpu() {
        let _ = gpu;
    }
    unsafe { kpi::hamix_free_irq() };
    kpi::dev_info("suspended, interrupt released");
}

fn resume() -> i32 {
    kpi::dev_info("resumed");
    0
}

hamix_kpi::module!(init = init, exit = exit, version = "1.1", suspend = suspend, resume = resume);
