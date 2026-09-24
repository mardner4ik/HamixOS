use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

use super::wifi::Security;
use super::{Bss, Kind, NetDevice, WifiControl};

pub const NET_ABI: u32 = 1;
pub const NET_ABI_WIFI: u32 = 2;
pub const FLAG_WIFI: u32 = 1;
const QUEUE_LIMIT: usize = 512;
const SCAN_MAX: usize = 64;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct NetOps {
    pub abi: u32,
    pub flags: u32,
    pub context: u64,
    pub mac: [u8; 8],
    pub transmit: Option<extern "C" fn(u64, *const u8, usize) -> i32>,
    pub set_enabled: Option<extern "C" fn(u64, i32)>,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct WifiBss {
    pub ssid: [u8; 32],
    pub ssid_len: u32,
    pub bssid: [u8; 6],
    pub channel: u8,
    pub security: u8,
    pub signal: u32,
    pub _pad: u32,
    pub last_seen: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct WifiStatus {
    pub associated: u32,
    pub signal: u32,
    pub ssid_len: u32,
    pub text_len: u32,
    pub ssid: [u8; 32],
    pub text: [u8; 96],
}

impl WifiStatus {
    const fn empty() -> WifiStatus {
        WifiStatus { associated: 0, signal: 0, ssid_len: 0, text_len: 0, ssid: [0; 32], text: [0; 96] }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct WifiOps {
    pub scan: Option<extern "C" fn(u64)>,
    pub networks: Option<extern "C" fn(u64, *mut WifiBss, u32) -> i32>,
    pub connect: Option<extern "C" fn(u64, *const u8, usize, *const u8, usize) -> i32>,
    pub disconnect: Option<extern "C" fn(u64)>,
    pub status: Option<extern "C" fn(u64, *mut WifiStatus) -> i32>,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct NetOpsV2 {
    pub abi: u32,
    pub flags: u32,
    pub context: u64,
    pub mac: [u8; 8],
    pub transmit: Option<extern "C" fn(u64, *const u8, usize) -> i32>,
    pub set_enabled: Option<extern "C" fn(u64, i32)>,
    pub wifi: WifiOps,
}

impl From<NetOps> for NetOpsV2 {
    fn from(ops: NetOps) -> NetOpsV2 {
        NetOpsV2 {
            abi: NET_ABI,
            flags: ops.flags & !FLAG_WIFI,
            context: ops.context,
            mac: ops.mac,
            transmit: ops.transmit,
            set_enabled: ops.set_enabled,
            wifi: WifiOps { scan: None, networks: None, connect: None, disconnect: None, status: None },
        }
    }
}

struct Slot {
    id: u32,
    owner: String,
    carrier: bool,
    alive: bool,
    rx: VecDeque<Vec<u8>>,
    rx_packets: u64,
    tx_packets: u64,
    dropped: u64,
}

static SLOTS: Mutex<Vec<Slot>> = Mutex::new(Vec::new());
static PENDING: Mutex<Vec<Box<dyn NetDevice>>> = Mutex::new(Vec::new());
static NEXT_ID: Mutex<u32> = Mutex::new(1);

fn with_slot<R>(id: u32, f: impl FnOnce(&mut Slot) -> R) -> Option<R> {
    crate::arch::without_interrupts(|| SLOTS.lock().iter_mut().find(|s| s.id == id).map(f))
}

pub struct ModuleNet {
    id: u32,
    name: String,
    driver: String,
    ops: NetOpsV2,
}

impl ModuleNet {
    fn is_wifi(&self) -> bool {
        self.ops.abi == NET_ABI_WIFI && self.ops.flags & FLAG_WIFI != 0
    }

    fn alive(&self) -> bool {
        with_slot(self.id, |s| s.alive).unwrap_or(false)
    }

    fn wifi_status(&self) -> Option<WifiStatus> {
        if !self.alive() {
            return None;
        }
        let status = self.ops.wifi.status?;
        let mut out = WifiStatus::empty();
        if status(self.ops.context, &mut out) != 0 {
            return None;
        }
        Some(out)
    }
}

fn text_of(bytes: &[u8], len: u32) -> String {
    String::from_utf8_lossy(&bytes[..(len as usize).min(bytes.len())]).into_owned()
}

impl WifiControl for ModuleNet {
    fn scan(&mut self) {
        if let (true, Some(scan)) = (self.alive(), self.ops.wifi.scan) {
            scan(self.ops.context);
        }
    }

    fn networks(&self) -> Vec<Bss> {
        let Some(networks) = self.ops.wifi.networks.filter(|_| self.alive()) else {
            return Vec::new();
        };
        let mut raw = alloc::vec![WifiBss { ssid: [0; 32], ssid_len: 0, bssid: [0; 6], channel: 0, security: 0, signal: 0, _pad: 0, last_seen: 0 }; SCAN_MAX];
        let count = networks(self.ops.context, raw.as_mut_ptr(), SCAN_MAX as u32).clamp(0, SCAN_MAX as i32) as usize;
        raw.truncate(count);
        raw.into_iter()
            .map(|b| Bss {
                ssid: text_of(&b.ssid, b.ssid_len),
                bssid: b.bssid,
                channel: b.channel,
                signal: b.signal.min(100),
                security: Security::from_code(b.security),
                last_seen: b.last_seen,
            })
            .collect()
    }

    fn connect(&mut self, ssid: &str, passphrase: &str) -> Result<(), &'static str> {
        let connect = self.ops.wifi.connect.filter(|_| self.alive()).ok_or("the Wi-Fi module is not running")?;
        match connect(self.ops.context, ssid.as_ptr(), ssid.len(), passphrase.as_ptr(), passphrase.len()) {
            0 => Ok(()),
            -19 => Err("Wi-Fi is off"),
            -22 => Err("bad SSID"),
            -34 => Err("the passphrase must be 8..63 characters"),
            -5 => Err("the radio is not initialised"),
            _ => Err("the Wi-Fi module refused the connection"),
        }
    }

    fn disconnect(&mut self) {
        if let (true, Some(disconnect)) = (self.alive(), self.ops.wifi.disconnect) {
            disconnect(self.ops.context);
        }
    }

    fn ssid(&self) -> Option<String> {
        let status = self.wifi_status()?;
        if status.associated == 0 || status.ssid_len == 0 {
            return None;
        }
        Some(text_of(&status.ssid, status.ssid_len))
    }

    fn signal(&self) -> u32 {
        self.wifi_status().map(|s| s.signal.min(100)).unwrap_or(0)
    }

    fn associated(&self) -> bool {
        self.wifi_status().map(|s| s.associated != 0).unwrap_or(false)
    }
}

impl NetDevice for ModuleNet {
    fn name(&self) -> &str {
        &self.name
    }

    fn kind(&self) -> Kind {
        if self.is_wifi() { Kind::Wifi } else { Kind::Ethernet }
    }

    fn driver(&self) -> String {
        format!("{} (module)", self.driver)
    }

    fn mac(&self) -> [u8; 6] {
        let mut mac = [0u8; 6];
        mac.copy_from_slice(&self.ops.mac[..6]);
        mac
    }

    fn link_up(&mut self) -> bool {
        with_slot(self.id, |s| s.alive && s.carrier).unwrap_or(false)
    }

    fn receive(&mut self) -> Option<Vec<u8>> {
        with_slot(self.id, |s| {
            let frame = s.rx.pop_front();
            if frame.is_some() {
                s.rx_packets += 1;
            }
            frame
        })
        .flatten()
    }

    fn transmit(&mut self, frame: &[u8]) -> bool {
        if !with_slot(self.id, |s| s.alive).unwrap_or(false) {
            return false;
        }
        let Some(transmit) = self.ops.transmit else {
            return false;
        };
        let sent = transmit(self.ops.context, frame.as_ptr(), frame.len()) == 0;
        if sent {
            with_slot(self.id, |s| s.tx_packets += 1);
        }
        sent
    }

    fn state_text(&self) -> String {
        if self.is_wifi() {
            if let Some(status) = self.wifi_status().filter(|s| s.text_len > 0) {
                return text_of(&status.text, status.text_len);
            }
        }
        with_slot(self.id, |s| format!("module {}, {} dropped", s.owner, s.dropped)).unwrap_or_default()
    }

    fn wifi(&mut self) -> Option<&mut dyn WifiControl> {
        if self.is_wifi() { Some(self) } else { None }
    }

    fn wifi_ref(&self) -> Option<&dyn WifiControl> {
        if self.is_wifi() { Some(self) } else { None }
    }

    fn counters(&self) -> (u64, u64) {
        with_slot(self.id, |s| (s.rx_packets, s.tx_packets)).unwrap_or((0, 0))
    }

    fn set_enabled(&mut self, enabled: bool) {
        if let Some(f) = self.ops.set_enabled {
            f(self.ops.context, enabled as i32);
        }
    }
}

pub fn register(driver: &str, ops: NetOpsV2, owner: &str) -> i32 {
    if ops.transmit.is_none() {
        return -22;
    }
    let kind = if ops.abi == NET_ABI_WIFI && ops.flags & FLAG_WIFI != 0 { Kind::Wifi } else { Kind::Ethernet };
    let id = {
        let mut next = NEXT_ID.lock();
        let id = *next;
        *next += 1;
        id
    };
    crate::arch::without_interrupts(|| {
        SLOTS.lock().push(Slot {
            id,
            owner: String::from(owner),
            carrier: false,
            alive: true,
            rx: VecDeque::new(),
            rx_packets: 0,
            tx_packets: 0,
            dropped: 0,
        })
    });
    let started = super::DAEMON.load(core::sync::atomic::Ordering::Relaxed);
    let index = if started {
        super::with_devices(|d| d.iter().filter(|d| d.kind() == kind).count())
    } else {
        PENDING.lock().iter().filter(|d| d.kind() == kind).count() + super::with_devices(|d| d.iter().filter(|d| d.kind() == kind).count())
    };
    let prefix = if kind == Kind::Wifi { "wlan" } else { "eth" };
    let device = ModuleNet { id, name: format!("{}{}", prefix, index), driver: String::from(driver), ops };
    crate::drivers::klog::log(&format!(
        "net: {} registered by module {} ({})",
        device.name,
        owner,
        super::dma::mac_text(&device.mac())
    ));
    if started {
        super::with_devices(|d| d.push(Box::new(device)));
    } else {
        PENDING.lock().push(Box::new(device));
    }
    id as i32
}

pub fn take_pending() -> Vec<Box<dyn NetDevice>> {
    core::mem::take(&mut *PENDING.lock())
}

pub fn unregister(id: u32) {
    with_slot(id, |s| {
        s.alive = false;
        s.carrier = false;
        s.rx.clear();
    });
}

pub fn forget_module(owner: &str) {
    crate::arch::without_interrupts(|| {
        for slot in SLOTS.lock().iter_mut().filter(|s| s.owner == owner) {
            slot.alive = false;
            slot.carrier = false;
            slot.rx.clear();
        }
    });
}

pub fn receive(id: u32, frame: &[u8]) {
    let copy = frame.to_vec();
    with_slot(id, |s| {
        if !s.alive {
            return;
        }
        if s.rx.len() >= QUEUE_LIMIT {
            s.dropped += 1;
            return;
        }
        s.rx.push_back(copy);
    });
}

pub fn carrier(id: u32, up: bool) {
    let changed = with_slot(id, |s| {
        let changed = s.carrier != up;
        s.carrier = up;
        changed
    })
    .unwrap_or(false);
    if changed {
        crate::drivers::klog::log(&format!("net: module device {} link {}", id, if up { "up" } else { "down" }));
    }
}
