pub mod dma;
pub mod inet;
pub mod known;
pub mod module;
pub mod stack;
pub mod unix;
pub mod wifi;

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use spin::Mutex;

use crate::hxinit::{self, fail, ok, skip, warn};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Ethernet,
    Wifi,
}

#[derive(Clone, Debug)]
pub struct Bss {
    pub ssid: String,
    pub bssid: [u8; 6],
    pub channel: u8,
    pub signal: u32,
    pub security: wifi::Security,
    pub last_seen: u64,
}

pub trait WifiControl {
    fn scan(&mut self);
    fn networks(&self) -> Vec<Bss>;
    fn connect(&mut self, ssid: &str, passphrase: &str) -> Result<(), &'static str>;
    fn disconnect(&mut self);
    fn ssid(&self) -> Option<String>;
    fn signal(&self) -> u32;
    fn associated(&self) -> bool;
}

pub trait NetDevice: Send {
    fn name(&self) -> &str;
    fn kind(&self) -> Kind;
    fn driver(&self) -> String;
    fn mac(&self) -> [u8; 6];
    fn link_up(&mut self) -> bool;
    fn receive(&mut self) -> Option<Vec<u8>>;
    fn transmit(&mut self, frame: &[u8]) -> bool;
    fn poll(&mut self) {}
    fn state_text(&self) -> String {
        String::new()
    }
    fn counters(&self) -> (u64, u64) {
        (0, 0)
    }
    fn wifi(&mut self) -> Option<&mut dyn WifiControl> {
        None
    }
    fn wifi_ref(&self) -> Option<&dyn WifiControl> {
        None
    }
    fn set_enabled(&mut self, _enabled: bool) {}
}

pub const MODE_OFF: u8 = 0;
pub const MODE_ETHERNET: u8 = 1;
pub const MODE_WIFI: u8 = 2;

pub static DEVICES: Mutex<Vec<Box<dyn NetDevice>>> = Mutex::new(Vec::new());
static MODE: AtomicU8 = AtomicU8::new(MODE_OFF);
pub(crate) static DAEMON: AtomicBool = AtomicBool::new(false);

pub fn mode() -> u8 {
    MODE.load(Ordering::Relaxed)
}

fn wanted_kind(mode: u8) -> Option<Kind> {
    match mode {
        MODE_ETHERNET => Some(Kind::Ethernet),
        MODE_WIFI => Some(Kind::Wifi),
        _ => None,
    }
}

pub fn with_devices<R>(f: impl FnOnce(&mut Vec<Box<dyn NetDevice>>) -> R) -> R {
    crate::arch::without_interrupts(|| f(&mut DEVICES.lock()))
}

pub fn set_mode(mode: u8) -> Result<(), &'static str> {
    if mode > MODE_WIFI {
        return Err("unknown mode");
    }
    let index = match wanted_kind(mode) {
        Some(kind) => Some(with_devices(|d| d.iter().position(|dev| dev.kind() == kind)).ok_or(if kind == Kind::Wifi { "no Wi-Fi adapter" } else { "no Ethernet adapter" })?),
        None => None,
    };
    MODE.store(mode, Ordering::Relaxed);
    known::save_mode(mode);
    with_devices(|devices| {
        for (i, dev) in devices.iter_mut().enumerate() {
            if dev.kind() == Kind::Wifi {
                dev.set_enabled(Some(i) == index);
            }
        }
    });
    stack::rebuild(index);
    crate::drivers::klog::log(&format!("net: mode {}", mode_name(mode)));
    Ok(())
}

pub fn mode_name(mode: u8) -> &'static str {
    match mode {
        MODE_ETHERNET => "ethernet",
        MODE_WIFI => "wifi",
        _ => "off",
    }
}

pub fn status_text() -> String {
    let mut out = format!("mode\t{}\n", mode_name(mode()));
    let active = stack::active_device();
    let config = stack::config();
    with_devices(|devices| {
        for (i, dev) in devices.iter_mut().enumerate() {
            let link = dev.link_up();
            let is_active = active == Some(i);
            let (address, gateway, dns, mut state) = match (&config, is_active) {
                (Some(c), true) => (c.address.clone(), c.gateway.clone(), c.dns.clone(), c.state.clone()),
                (None, true) => (String::new(), String::new(), String::new(), String::from("Starting…")),
                _ => (String::new(), String::new(), String::new(), String::from("inactive")),
            };
            let (ssid, signal) = match dev.wifi_ref() {
                Some(w) => (w.ssid().unwrap_or_default(), w.signal()),
                None => (String::new(), 0),
            };
            let extra = dev.state_text();
            if !extra.is_empty() && (address.is_empty() || !is_active) {
                state = extra;
            }
            let (rx, tx) = dev.counters();
            out.push_str(&format!(
                "iface\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                dev.name(),
                if dev.kind() == Kind::Wifi { "wifi" } else { "ethernet" },
                dev.driver(),
                dma::mac_text(&dev.mac()),
                if link { "up" } else { "down" },
                if is_active && link { address } else { String::new() },
                gateway,
                dns,
                state,
                ssid,
                signal,
                rx,
                tx
            ));
        }
    });
    out
}

pub fn wifi_scan_text(refresh: bool) -> Result<String, &'static str> {
    with_devices(|devices| {
        let dev = devices.iter_mut().find(|d| d.kind() == Kind::Wifi).ok_or("no Wi-Fi adapter")?;
        let wifi = dev.wifi().ok_or("no Wi-Fi adapter")?;
        if refresh {
            wifi.scan();
        }
        let current = wifi.ssid();
        let associated = wifi.associated();
        let mut out = String::new();
        for bss in wifi.networks() {
            out.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\t{}\n",
                bss.ssid.replace('\t', " "),
                dma::mac_text(&bss.bssid),
                bss.channel,
                bss.signal,
                bss.security.name(),
                if associated && current.as_deref() == Some(bss.ssid.as_str()) { 1 } else { 0 }
            ));
        }
        Ok(out)
    })
}

pub fn wifi_connect(ssid: &str, passphrase: &str) -> Result<(), &'static str> {
    if mode() != MODE_WIFI {
        set_mode(MODE_WIFI)?;
    }
    let result = with_devices(|devices| {
        let dev = devices.iter_mut().find(|d| d.kind() == Kind::Wifi).ok_or("no Wi-Fi adapter")?;
        dev.wifi().ok_or("no Wi-Fi adapter")?.connect(ssid, passphrase)
    });
    if result.is_ok() {
        known::remember(ssid, passphrase);
    }
    result
}

static LAST_AUTOCONNECT: AtomicU64 = AtomicU64::new(0);

const AUTOCONNECT_INTERVAL_MS: u64 = 8000;

fn autoconnect() {
    if mode() != MODE_WIFI {
        return;
    }
    let now = crate::task::uptime_ms();
    if now.saturating_sub(LAST_AUTOCONNECT.load(Ordering::Relaxed)) < AUTOCONNECT_INTERVAL_MS {
        return;
    }
    let idle = with_devices(|devices| match devices.iter_mut().find(|d| d.kind() == Kind::Wifi).and_then(|d| d.wifi()) {
        Some(wifi) => !wifi.associated(),
        None => false,
    });
    if !idle {
        return;
    }
    LAST_AUTOCONNECT.store(now, Ordering::Relaxed);
    let saved = known::list();
    if saved.is_empty() {
        return;
    }
    let visible = with_devices(|devices| match devices.iter_mut().find(|d| d.kind() == Kind::Wifi).and_then(|d| d.wifi()) {
        Some(wifi) => {
            let list = wifi.networks();
            if list.is_empty() {
                wifi.scan();
            }
            list
        }
        None => Vec::new(),
    });
    let mut candidates: Vec<(u32, String, String)> = Vec::new();
    for bss in visible.iter() {
        if let Some(entry) = saved.iter().find(|e| e.ssid == bss.ssid && e.automatic) {
            candidates.push((bss.signal, entry.ssid.clone(), entry.passphrase.clone()));
        }
    }
    candidates.sort_by(|a, b| b.0.cmp(&a.0));
    let Some((_, ssid, passphrase)) = candidates.first() else {
        return;
    };
    let attempted = with_devices(|devices| {
        let Some(wifi) = devices.iter_mut().find(|d| d.kind() == Kind::Wifi).and_then(|d| d.wifi()) else {
            return false;
        };
        wifi.connect(ssid, passphrase).is_ok()
    });
    if attempted {
        crate::drivers::klog::log(&format!("net: reconnecting to the saved network {}", ssid));
    }
}

pub fn wifi_forget(ssid: &str) -> bool {
    known::forget(ssid)
}

pub fn wifi_known_text() -> String {
    known::listing_text()
}

pub fn wifi_disconnect() -> Result<(), &'static str> {
    with_devices(|devices| {
        let dev = devices.iter_mut().find(|d| d.kind() == Kind::Wifi).ok_or("no Wi-Fi adapter")?;
        dev.wifi().ok_or("no Wi-Fi adapter")?.disconnect();
        Ok(())
    })
}

fn write_resolv_conf() {
    let servers = stack::dns_servers();
    if servers.is_empty() {
        return;
    }
    let mut text = String::from("# written by HamixOS from the DHCP lease\n");
    for s in servers.iter() {
        text.push_str(&format!("nameserver {}.{}.{}.{}\n", s[0], s[1], s[2], s[3]));
    }
    let mut guard = crate::fs::VFS.lock();
    let Some(vfs) = guard.as_mut() else {
        return;
    };
    let _ = vfs.write(0, "/etc/resolv.conf", text.as_bytes(), false, 0);
    let root = vfs.root_id();
    if vfs.resolve(root, "/opt/linux/etc/resolv.conf").is_some() {
        let _ = vfs.write(0, "/opt/linux/etc/resolv.conf", text.as_bytes(), false, 0);
    }
}

extern "C" fn daemon(_: u64) -> ! {
    loop {
        crate::module::poll_devices();
        let busy = stack::poll();
        if inet::open_count() > 0 && stack::take_changed() {
            crate::task::wake_all(crate::task::WAIT_PIPE);
        }
        if stack::take_dns_dirty() {
            write_resolv_conf();
        }
        autoconnect();
        let pause = if busy || inet::open_count() > 0 { 1 } else { 10 };
        crate::task::sleep_ticks(pause);
    }
}

fn probe_devices() -> Vec<Box<dyn NetDevice>> {
    let mut found: Vec<Box<dyn NetDevice>> = module::take_pending();
    crate::drivers::virtio::net::probe(&mut found);
    found
}

pub fn init_units() {
    let mut found = Vec::new();
    hxinit::run("netdev", "Network adapters", || {
        found = probe_devices();
        let unsupported: Vec<String> = crate::drivers::pci::devices()
            .iter()
            .filter(|d| d.class == 0x02)
            .map(|d| format!("{:04x}:{:04x}", d.vendor, d.device))
            .collect();
        if found.is_empty() {
            if unsupported.is_empty() { skip("no network controllers") } else { warn(format!("no driver for {}", unsupported.join(", "))) }
        } else {
            ok(found.iter().map(|d| format!("{} {} {}", d.name(), d.driver(), dma::mac_text(&d.mac()))).collect::<Vec<_>>().join(", "))
        }
    });
    let has_ethernet = found.iter().any(|d| d.kind() == Kind::Ethernet);
    let has_wifi = found.iter().any(|d| d.kind() == Kind::Wifi);
    with_devices(|d| *d = found);
    hxinit::run("netstack", "TCP/IP stack (smoltcp)", || {
        let default_mode = if has_ethernet { MODE_ETHERNET } else if has_wifi { MODE_WIFI } else { MODE_OFF };
        let initial = match known::saved_mode() {
            Some(MODE_WIFI) if has_wifi => MODE_WIFI,
            Some(MODE_ETHERNET) if has_ethernet => MODE_ETHERNET,
            Some(MODE_OFF) => MODE_OFF,
            _ => default_mode,
        };
        match set_mode(initial) {
            Ok(()) => {
                if !DAEMON.swap(true, Ordering::Relaxed) {
                    crate::task::spawn_kernel_thread("netd", 0, daemon, 0);
                }
                if initial == MODE_OFF { skip("no adapter, stack idle") } else { ok(format!("smoltcp 0.12, mode {}, DHCP client started", mode_name(initial))) }
            }
            Err(e) => fail(e),
        }
    });
}

pub fn cleanup(pid: crate::task::Pid) {
    stack::close_all(pid);
}

pub struct IfInfo {
    pub index: u32,
    pub name: String,
    pub mac: [u8; 6],
    pub up: bool,
    pub loopback: bool,
    pub address: Option<([u8; 4], u8)>,
    pub rx: u64,
    pub tx: u64,
}

pub fn interfaces() -> Vec<IfInfo> {
    let mut out = alloc::vec![IfInfo { index: 1, name: String::from("lo"), mac: [0; 6], up: true, loopback: true, address: Some(([127, 0, 0, 1], 8)), rx: 0, tx: 0 }];
    let active = stack::active_device();
    let address = stack::ipv4();
    with_devices(|devices| {
        for (i, dev) in devices.iter_mut().enumerate() {
            let up = dev.link_up();
            let (rx, tx) = dev.counters();
            out.push(IfInfo {
                index: i as u32 + 2,
                name: String::from(dev.name()),
                mac: dev.mac(),
                up,
                loopback: false,
                address: if active == Some(i) { address } else { None },
                rx,
                tx,
            });
        }
    });
    out
}
