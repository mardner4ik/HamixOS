#![no_std]

extern crate alloc;

pub mod crypto;
pub mod frame;

use alloc::collections::VecDeque;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use frame::*;
pub use frame::Security;

pub trait Radio {
    fn mac(&self) -> [u8; 6];
    fn set_channel(&mut self, channel: u8) -> bool;
    fn set_bssid(&mut self, bssid: Option<[u8; 6]>);
    fn transmit(&mut self, frame: &[u8], management: bool) -> bool;
}

#[derive(Clone, Debug)]
pub struct Bss {
    pub ssid: String,
    pub bssid: [u8; 6],
    pub channel: u8,
    pub rssi: i8,
    pub security: Security,
    pub rsn_ie: Vec<u8>,
    pub last_seen: u64,
}

impl Bss {
    pub fn signal_percent(&self) -> u32 {
        ((self.rssi as i32 + 95) * 100 / 60).clamp(0, 100) as u32
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Disabled,
    Idle,
    Scanning,
    Authenticating,
    Associating,
    Handshake,
    Connected,
}

struct Keys {
    kck: [u8; 16],
    kek: [u8; 16],
    tk: [u8; 16],
    gtk: Option<(u8, [u8; 16])>,
    tx_pn: u64,
    rx_pn: [u64; 4],
    rx_pn_unicast: u64,
}

struct Target {
    ssid: String,
    pmk: Option<[u8; 32]>,
}

pub struct Station {
    pub state: State,
    bss: Vec<Bss>,
    target: Option<Target>,
    current: Option<Bss>,
    scan_channels: Vec<u8>,
    scan_index: usize,
    dwell_until: u64,
    deadline: u64,
    attempts: u32,
    sequence: u16,
    anonce: [u8; 32],
    snonce: [u8; 32],
    replay: u64,
    ptk: Option<Keys>,
    last_beacon: u64,
    received: VecDeque<Vec<u8>>,
    rng: u64,
    pub message: String,
    pub rssi: i8,
    connect_after_scan: bool,
}

const SCAN_DWELL_MS: u64 = 110;
const AUTH_TIMEOUT_MS: u64 = 400;
const HANDSHAKE_TIMEOUT_MS: u64 = 4000;
const BEACON_LOSS_MS: u64 = 10_000;

fn mac_text(mac: &[u8; 6]) -> String {
    format!("{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}", mac[0], mac[1], mac[2], mac[3], mac[4], mac[5])
}

impl Station {
    pub fn new(seed: u64) -> Station {
        Station {
            state: State::Disabled,
            bss: Vec::new(),
            target: None,
            current: None,
            scan_channels: Vec::new(),
            scan_index: 0,
            dwell_until: 0,
            deadline: 0,
            attempts: 0,
            sequence: 0,
            anonce: [0; 32],
            snonce: [0; 32],
            replay: 0,
            ptk: None,
            last_beacon: 0,
            received: VecDeque::new(),
            rng: seed | 1,
            message: String::new(),
            rssi: -100,
            connect_after_scan: false,
        }
    }

    fn random(&mut self) -> u64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.rng
    }

    fn next_sequence(&mut self) -> u16 {
        self.sequence = (self.sequence + 1) & 0x0FFF;
        self.sequence
    }

    pub fn enable(&mut self, on: bool, radio: &mut dyn Radio) {
        if on && self.state == State::Disabled {
            self.state = State::Idle;
            self.start_scan(0, radio);
        } else if !on {
            self.disconnect(radio);
            self.state = State::Disabled;
        }
    }

    pub fn networks(&self) -> Vec<Bss> {
        let mut list: Vec<Bss> = Vec::new();
        for bss in self.bss.iter() {
            match list.iter_mut().find(|b| b.ssid == bss.ssid && !bss.ssid.is_empty()) {
                Some(existing) => {
                    if bss.rssi > existing.rssi {
                        *existing = bss.clone();
                    }
                }
                None => list.push(bss.clone()),
            }
        }
        list.sort_by(|a, b| b.rssi.cmp(&a.rssi));
        list
    }

    pub fn associated(&self) -> bool {
        self.state == State::Connected
    }

    pub fn ssid(&self) -> Option<String> {
        self.current.as_ref().map(|b| b.ssid.clone()).or_else(|| self.target.as_ref().map(|t| t.ssid.clone()))
    }

    pub fn signal(&self) -> u32 {
        if self.state == State::Connected { ((self.rssi as i32 + 95) * 100 / 60).clamp(0, 100) as u32 } else { 0 }
    }

    pub fn status_text(&self) -> String {
        match self.state {
            State::Disabled => String::from("Wi-Fi is off"),
            State::Idle => if self.message.is_empty() { String::from("not connected") } else { self.message.clone() },
            State::Scanning => String::from("scanning…"),
            State::Authenticating => format!("authenticating with {}…", self.ssid().unwrap_or_default()),
            State::Associating => format!("associating with {}…", self.ssid().unwrap_or_default()),
            State::Handshake => String::from("negotiating keys (WPA2)…"),
            State::Connected => format!("connected to {}", self.ssid().unwrap_or_default()),
        }
    }

    pub fn start_scan(&mut self, now: u64, radio: &mut dyn Radio) {
        if self.state == State::Disabled {
            return;
        }
        if matches!(self.state, State::Authenticating | State::Associating | State::Handshake | State::Connected) {
            return;
        }
        self.scan_channels = (1..=13).collect();
        self.scan_index = 0;
        self.state = State::Scanning;
        self.bss.retain(|b| now.saturating_sub(b.last_seen) < 60_000);
        self.scan_step(now, radio);
    }

    fn scan_step(&mut self, now: u64, radio: &mut dyn Radio) {
        if self.scan_index >= self.scan_channels.len() {
            self.state = State::Idle;
            if self.connect_after_scan {
                self.connect_after_scan = false;
                self.begin_auth(now, radio);
            }
            return;
        }
        let channel = self.scan_channels[self.scan_index];
        self.scan_index += 1;
        radio.set_bssid(None);
        radio.set_channel(channel);
        let mut probe = Vec::with_capacity(64);
        let seq = self.next_sequence();
        let own = radio.mac();
        build_header(&mut probe, TYPE_MGMT, SUB_PROBE_REQ, 0, BROADCAST, own, BROADCAST, seq);
        probe.extend_from_slice(&[IE_SSID, 0]);
        probe.push(IE_RATES);
        probe.push(RATES.len() as u8);
        probe.extend_from_slice(&RATES);
        probe.push(IE_EXT_RATES);
        probe.push(EXT_RATES.len() as u8);
        probe.extend_from_slice(&EXT_RATES);
        radio.transmit(&probe, true);
        self.dwell_until = now + SCAN_DWELL_MS;
    }

    pub fn connect(&mut self, ssid: &str, passphrase: &str, now: u64, radio: &mut dyn Radio) -> Result<(), &'static str> {
        if self.state == State::Disabled {
            return Err("Wi-Fi is off");
        }
        if ssid.is_empty() || ssid.len() > 32 {
            return Err("bad SSID");
        }
        let pmk = if passphrase.is_empty() {
            None
        } else {
            if passphrase.len() < 8 || passphrase.len() > 63 {
                return Err("the passphrase must be 8..63 characters");
            }
            Some(crypto::psk_from_passphrase(passphrase, ssid.as_bytes()))
        };
        self.disconnect(radio);
        self.target = Some(Target { ssid: String::from(ssid), pmk });
        self.attempts = 0;
        self.message.clear();
        if self.bss.iter().any(|b| b.ssid == ssid) {
            self.begin_auth(now, radio);
        } else {
            self.connect_after_scan = true;
            self.state = State::Idle;
            self.start_scan(now, radio);
        }
        Ok(())
    }

    pub fn disconnect(&mut self, radio: &mut dyn Radio) {
        if let Some(bss) = self.current.take() {
            if matches!(self.state, State::Associating | State::Handshake | State::Connected) {
                let mut deauth = Vec::new();
                let seq = self.next_sequence();
                let own = radio.mac();
                build_header(&mut deauth, TYPE_MGMT, SUB_DEAUTH, 0, bss.bssid, own, bss.bssid, seq);
                deauth.extend_from_slice(&3u16.to_le_bytes());
                radio.transmit(&deauth, true);
            }
        }
        self.ptk = None;
        self.target = None;
        self.connect_after_scan = false;
        radio.set_bssid(None);
        if self.state != State::Disabled {
            self.state = State::Idle;
        }
    }

    fn fail(&mut self, reason: &str, now: u64, radio: &mut dyn Radio) {
        self.attempts += 1;
        self.ptk = None;
        if self.attempts >= 3 {
            self.message = format!("connection failed: {}", reason);
            self.current = None;
            self.target = None;
            self.state = State::Idle;
            radio.set_bssid(None);
        } else {
            self.begin_auth(now, radio);
        }
    }

    fn begin_auth(&mut self, now: u64, radio: &mut dyn Radio) {
        let Some(target) = self.target.as_ref() else {
            self.state = State::Idle;
            return;
        };
        let Some(bss) = self.bss.iter().filter(|b| b.ssid == target.ssid).max_by_key(|b| b.rssi).cloned() else {
            self.message = format!("network {} not found", target.ssid);
            self.state = State::Idle;
            self.target = None;
            return;
        };
        if bss.security == Security::Wpa2 && (target.pmk.is_none() || !rsn_supports_ccmp_psk(&bss.rsn_ie)) {
            self.message = String::from(if target.pmk.is_none() { "this network needs a password" } else { "only WPA2-PSK with CCMP is supported" });
            self.state = State::Idle;
            self.target = None;
            return;
        }
        if matches!(bss.security, Security::Wep | Security::Wpa) {
            self.message = String::from("WEP and WPA1 networks are not supported");
            self.state = State::Idle;
            self.target = None;
            return;
        }
        radio.set_channel(bss.channel);
        radio.set_bssid(Some(bss.bssid));
        let mut auth = Vec::new();
        let seq = self.next_sequence();
        let own = radio.mac();
        build_header(&mut auth, TYPE_MGMT, SUB_AUTH, 0, bss.bssid, own, bss.bssid, seq);
        auth.extend_from_slice(&0u16.to_le_bytes());
        auth.extend_from_slice(&1u16.to_le_bytes());
        auth.extend_from_slice(&0u16.to_le_bytes());
        radio.transmit(&auth, true);
        self.current = Some(bss);
        self.state = State::Authenticating;
        self.deadline = now + AUTH_TIMEOUT_MS;
    }

    fn send_assoc(&mut self, now: u64, radio: &mut dyn Radio) {
        let Some(bss) = self.current.clone() else {
            return;
        };
        let mut req = Vec::new();
        let seq = self.next_sequence();
        let own = radio.mac();
        build_header(&mut req, TYPE_MGMT, SUB_ASSOC_REQ, 0, bss.bssid, own, bss.bssid, seq);
        let mut capability: u16 = 0x0001 | 0x0020 | 0x0400;
        if bss.security != Security::Open {
            capability |= 0x0010;
        }
        req.extend_from_slice(&capability.to_le_bytes());
        req.extend_from_slice(&10u16.to_le_bytes());
        req.push(IE_SSID);
        req.push(bss.ssid.len() as u8);
        req.extend_from_slice(bss.ssid.as_bytes());
        req.push(IE_RATES);
        req.push(RATES.len() as u8);
        req.extend_from_slice(&RATES);
        req.push(IE_EXT_RATES);
        req.push(EXT_RATES.len() as u8);
        req.extend_from_slice(&EXT_RATES);
        if bss.security == Security::Wpa2 {
            req.push(IE_RSN);
            req.push(RSN_IE_CCMP_PSK.len() as u8);
            req.extend_from_slice(&RSN_IE_CCMP_PSK);
        }
        radio.transmit(&req, true);
        self.state = State::Associating;
        self.deadline = now + AUTH_TIMEOUT_MS;
    }

    pub fn tick(&mut self, now: u64, radio: &mut dyn Radio) {
        match self.state {
            State::Scanning if now >= self.dwell_until => self.scan_step(now, radio),
            State::Authenticating | State::Associating if now >= self.deadline => {
                let what = if self.state == State::Authenticating { "no authentication response" } else { "no association response" };
                self.fail(what, now, radio);
            }
            State::Handshake if now >= self.deadline => self.fail("the WPA2 handshake timed out (wrong password?)", now, radio),
            State::Connected if now.saturating_sub(self.last_beacon) > BEACON_LOSS_MS => {
                self.message = String::from("the access point went away");
                self.ptk = None;
                self.state = State::Idle;
                self.attempts = 0;
                if self.target.is_some() {
                    self.begin_auth(now, radio);
                }
            }
            _ => {}
        }
    }

    pub fn on_frame(&mut self, mpdu: &[u8], rssi: i8, now: u64, radio: &mut dyn Radio) {
        let Some(h) = parse(mpdu) else {
            return;
        };
        let own = radio.mac();
        match (h.frame_type, h.subtype) {
            (TYPE_MGMT, SUB_BEACON) | (TYPE_MGMT, SUB_PROBE_RESP) => {
                let Some(info) = parse_beacon(h.body) else {
                    return;
                };
                let bssid = h.addr3;
                match self.bss.iter_mut().find(|b| b.bssid == bssid) {
                    Some(b) => {
                        b.rssi = rssi;
                        b.last_seen = now;
                        if !info.ssid.is_empty() {
                            b.ssid = info.ssid.clone();
                        }
                        b.security = info.security;
                        b.rsn_ie = info.rsn_ie.clone();
                        if info.channel != 0 {
                            b.channel = info.channel;
                        }
                    }
                    None => self.bss.push(Bss { ssid: info.ssid.clone(), bssid, channel: info.channel, rssi, security: info.security, rsn_ie: info.rsn_ie.clone(), last_seen: now }),
                }
                if self.current.as_ref().map(|c| c.bssid == bssid).unwrap_or(false) {
                    self.last_beacon = now;
                    self.rssi = rssi;
                }
            }
            (TYPE_MGMT, SUB_AUTH) if h.addr1 == own && self.state == State::Authenticating => {
                if h.body.len() >= 6 {
                    let seq = u16::from_le_bytes([h.body[2], h.body[3]]);
                    let status = u16::from_le_bytes([h.body[4], h.body[5]]);
                    if seq == 2 && status == 0 {
                        self.send_assoc(now, radio);
                    } else if seq == 2 {
                        self.fail("authentication refused", now, radio);
                    }
                }
            }
            (TYPE_MGMT, SUB_ASSOC_RESP) if h.addr1 == own && self.state == State::Associating => {
                if h.body.len() >= 6 {
                    let status = u16::from_le_bytes([h.body[2], h.body[3]]);
                    if status != 0 {
                        self.fail("association refused", now, radio);
                        return;
                    }
                    self.last_beacon = now;
                    let secured = self.current.as_ref().map(|b| b.security == Security::Wpa2).unwrap_or(false);
                    if secured {
                        self.state = State::Handshake;
                        self.deadline = now + HANDSHAKE_TIMEOUT_MS;
                    } else {
                        self.state = State::Connected;
                        self.attempts = 0;
                        self.message.clear();
                    }
                }
            }
            (TYPE_MGMT, SUB_DEAUTH) | (TYPE_MGMT, SUB_DISASSOC) => {
                if h.addr1 == own && self.current.as_ref().map(|b| b.bssid == h.addr2).unwrap_or(false) {
                    self.ptk = None;
                    if matches!(self.state, State::Connected | State::Handshake) {
                        self.message = String::from("disconnected by the access point");
                        self.state = State::Idle;
                        self.attempts = 0;
                        self.begin_auth(now, radio);
                    }
                }
            }
            (TYPE_DATA, subtype) if h.flags & FC_FROM_DS != 0 && h.flags & FC_TO_DS == 0 => {
                if subtype & 0x04 != 0 {
                    return;
                }
                if !self.current.as_ref().map(|b| b.bssid == h.addr2).unwrap_or(false) {
                    return;
                }
                if h.addr1 != own && h.addr1[0] & 1 == 0 {
                    return;
                }
                let payload = if h.flags & FC_PROTECTED != 0 {
                    match self.decrypt(mpdu, &h) {
                        Some(p) => p,
                        None => return,
                    }
                } else {
                    h.body.to_vec()
                };
                if payload.len() < 8 || payload[..6] != [0xAA, 0xAA, 0x03, 0x00, 0x00, 0x00] {
                    return;
                }
                let ethertype = [payload[6], payload[7]];
                let data = &payload[8..];
                if ethertype == [0x88, 0x8E] {
                    self.on_eapol(data, h.flags & FC_PROTECTED != 0, now, radio);
                    return;
                }
                if self.state != State::Connected {
                    return;
                }
                let mut eth = Vec::with_capacity(14 + data.len());
                eth.extend_from_slice(&h.addr1);
                eth.extend_from_slice(&h.addr3);
                eth.extend_from_slice(&ethertype);
                eth.extend_from_slice(data);
                if self.received.len() < 256 {
                    self.received.push_back(eth);
                }
            }
            _ => {}
        }
    }

    fn decrypt(&mut self, mpdu: &[u8], h: &Header) -> Option<Vec<u8>> {
        let keys = self.ptk.as_mut()?;
        let body = h.body;
        if body.len() < 16 || body[3] & 0x20 == 0 {
            return None;
        }
        let key_id = body[3] >> 6;
        let pn = u64::from_le_bytes([body[0], body[1], body[4], body[5], body[6], body[7], 0, 0]);
        let group = h.addr1[0] & 1 != 0;
        let key = if group {
            match keys.gtk {
                Some((id, k)) if id == key_id => k,
                _ => return None,
            }
        } else {
            keys.tk
        };
        let last = if group { keys.rx_pn[key_id as usize] } else { keys.rx_pn_unicast };
        if pn <= last && last != 0 {
            return None;
        }
        let (nonce, aad) = ccmp_nonce_aad(&mpdu[..h.header_len], pn);
        let plain = crypto::ccm_decrypt(&key, &nonce, &aad, &body[8..])?;
        if group {
            keys.rx_pn[key_id as usize] = pn;
        } else {
            keys.rx_pn_unicast = pn;
        }
        Some(plain)
    }

    fn eapol_key_frame(&self, key_info: u16, replay: u64, nonce: &[u8; 32], key_data: &[u8]) -> Vec<u8> {
        let mut body = Vec::with_capacity(99 + key_data.len());
        let length = 95 + key_data.len();
        body.extend_from_slice(&[0x02, 0x03]);
        body.extend_from_slice(&(length as u16).to_be_bytes());
        body.push(0x02);
        body.extend_from_slice(&key_info.to_be_bytes());
        body.extend_from_slice(&0u16.to_be_bytes());
        body.extend_from_slice(&replay.to_be_bytes());
        body.extend_from_slice(nonce);
        body.extend_from_slice(&[0u8; 16]);
        body.extend_from_slice(&[0u8; 8]);
        body.extend_from_slice(&[0u8; 8]);
        body.extend_from_slice(&[0u8; 16]);
        body.extend_from_slice(&(key_data.len() as u16).to_be_bytes());
        body.extend_from_slice(key_data);
        body
    }

    fn send_eapol(&mut self, mut eapol: Vec<u8>, kck: &[u8; 16], radio: &mut dyn Radio) {
        let mic = crypto::hmac_sha1(kck, &[&eapol]);
        eapol[81..97].copy_from_slice(&mic[..16]);
        let mut payload = Vec::with_capacity(8 + eapol.len());
        payload.extend_from_slice(&[0xAA, 0xAA, 0x03, 0x00, 0x00, 0x00, 0x88, 0x8E]);
        payload.extend_from_slice(&eapol);
        let Some(bss) = self.current.clone() else {
            return;
        };
        let own = radio.mac();
        self.send_data(bss.bssid, own, bss.bssid, &payload, radio);
    }

    fn on_eapol(&mut self, data: &[u8], _protected: bool, now: u64, radio: &mut dyn Radio) {
        if data.len() < 99 || data[1] != 3 || data[4] != 2 {
            return;
        }
        let key_info = u16::from_be_bytes([data[5], data[6]]);
        let replay = u64::from_be_bytes(data[9..17].try_into().unwrap());
        let nonce: [u8; 32] = data[17..49].try_into().unwrap();
        let mic: [u8; 16] = data[81..97].try_into().unwrap();
        let data_len = u16::from_be_bytes([data[97], data[98]]) as usize;
        if data.len() < 99 + data_len {
            return;
        }
        let key_data = &data[99..99 + data_len];
        let pairwise = key_info & (1 << 3) != 0;
        let has_mic = key_info & (1 << 8) != 0;
        let ack = key_info & (1 << 7) != 0;
        let Some(bss) = self.current.clone() else {
            return;
        };
        let Some(pmk) = self.target.as_ref().and_then(|t| t.pmk) else {
            return;
        };
        let own = radio.mac();
        if pairwise && ack && !has_mic {
            if replay < self.replay && self.replay != 0 {
                return;
            }
            self.replay = replay;
            self.anonce = nonce;
            let mut snonce = [0u8; 32];
            for chunk in snonce.chunks_mut(8) {
                let r = self.random().to_le_bytes();
                chunk.copy_from_slice(&r[..chunk.len()]);
            }
            self.snonce = snonce;
            let (min_mac, max_mac) = if own < bss.bssid { (own, bss.bssid) } else { (bss.bssid, own) };
            let (min_nonce, max_nonce) = if self.snonce < self.anonce { (self.snonce, self.anonce) } else { (self.anonce, self.snonce) };
            let mut seed = Vec::with_capacity(76);
            seed.extend_from_slice(&min_mac);
            seed.extend_from_slice(&max_mac);
            seed.extend_from_slice(&min_nonce);
            seed.extend_from_slice(&max_nonce);
            let mut ptk = [0u8; 48];
            crypto::prf_80211(&pmk, b"Pairwise key expansion", &seed, &mut ptk);
            let keys = Keys {
                kck: ptk[0..16].try_into().unwrap(),
                kek: ptk[16..32].try_into().unwrap(),
                tk: ptk[32..48].try_into().unwrap(),
                gtk: None,
                tx_pn: 0,
                rx_pn: [0; 4],
                rx_pn_unicast: 0,
            };
            let kck = keys.kck;
            self.ptk = Some(keys);
            let mut rsn = alloc::vec![IE_RSN, RSN_IE_CCMP_PSK.len() as u8];
            rsn.extend_from_slice(&RSN_IE_CCMP_PSK);
            let snonce = self.snonce;
            let frame = self.eapol_key_frame(0x010A, replay, &snonce, &rsn);
            self.send_eapol(frame, &kck, radio);
            self.state = State::Handshake;
            self.deadline = now + HANDSHAKE_TIMEOUT_MS;
            return;
        }
        let Some(keys) = self.ptk.as_ref() else {
            return;
        };
        if !has_mic {
            return;
        }
        let mut check = data[..99 + data_len].to_vec();
        check[81..97].fill(0);
        let expected = crypto::hmac_sha1(&keys.kck, &[&check]);
        if expected[..16] != mic {
            self.message = String::from("wrong password (MIC mismatch)");
            return;
        }
        if replay < self.replay {
            return;
        }
        self.replay = replay;
        let kek = keys.kek;
        let kck = keys.kck;
        let decrypted = if key_info & (1 << 12) != 0 { crypto::aes_unwrap(&kek, key_data) } else { Some(key_data.to_vec()) };
        let Some(plain) = decrypted else {
            return;
        };
        let gtk = find_gtk(&plain);
        if pairwise {
            if nonce != self.anonce {
                return;
            }
            let frame = self.eapol_key_frame(0x030A, replay, &[0u8; 32], &[]);
            self.send_eapol(frame, &kck, radio);
            if let Some(keys) = self.ptk.as_mut() {
                keys.gtk = gtk;
            }
            self.state = State::Connected;
            self.attempts = 0;
            self.message.clear();
            self.last_beacon = now;
        } else {
            if let (Some(g), Some(keys)) = (gtk, self.ptk.as_mut()) {
                keys.gtk = Some(g);
                keys.rx_pn[g.0 as usize & 3] = 0;
            }
            let frame = self.eapol_key_frame(0x0302, replay, &[0u8; 32], &[]);
            self.send_eapol(frame, &kck, radio);
        }
    }

    fn send_data(&mut self, addr1: [u8; 6], addr2: [u8; 6], addr3: [u8; 6], payload: &[u8], radio: &mut dyn Radio) -> bool {
        let seq = self.next_sequence();
        let encrypt = self.state == State::Connected && self.ptk.is_some();
        let mut frame = Vec::with_capacity(32 + payload.len() + 16);
        build_header(&mut frame, TYPE_DATA, 0, FC_TO_DS | if encrypt { FC_PROTECTED } else { 0 }, addr1, addr2, addr3, seq);
        if encrypt {
            let keys = self.ptk.as_mut().unwrap();
            keys.tx_pn += 1;
            let pn = keys.tx_pn;
            let tk = keys.tk;
            let pn_bytes = pn.to_le_bytes();
            let (nonce, aad) = ccmp_nonce_aad(&frame[..24], pn);
            frame.extend_from_slice(&[pn_bytes[0], pn_bytes[1], 0, 0x20, pn_bytes[2], pn_bytes[3], pn_bytes[4], pn_bytes[5]]);
            frame.extend_from_slice(&crypto::ccm_encrypt(&tk, &nonce, &aad, payload));
        } else {
            frame.extend_from_slice(payload);
        }
        radio.transmit(&frame, false)
    }

    pub fn send_ethernet(&mut self, eth: &[u8], radio: &mut dyn Radio) -> bool {
        if self.state != State::Connected || eth.len() < 14 {
            return false;
        }
        let Some(bss) = self.current.clone() else {
            return false;
        };
        let dst: [u8; 6] = eth[0..6].try_into().unwrap();
        let src: [u8; 6] = eth[6..12].try_into().unwrap();
        let mut payload = Vec::with_capacity(eth.len() - 6);
        payload.extend_from_slice(&[0xAA, 0xAA, 0x03, 0x00, 0x00, 0x00, eth[12], eth[13]]);
        payload.extend_from_slice(&eth[14..]);
        self.send_data(bss.bssid, src, dst, &payload, radio)
    }

    pub fn take_ethernet(&mut self) -> Option<Vec<u8>> {
        self.received.pop_front()
    }

    pub fn bssid_text(&self) -> String {
        self.current.as_ref().map(|b| mac_text(&b.bssid)).unwrap_or_default()
    }
}

fn find_gtk(key_data: &[u8]) -> Option<(u8, [u8; 16])> {
    let mut i = 0;
    while i + 2 <= key_data.len() {
        let id = key_data[i];
        let len = key_data[i + 1] as usize;
        if id == 0 && len == 0 {
            break;
        }
        if i + 2 + len > key_data.len() {
            break;
        }
        let value = &key_data[i + 2..i + 2 + len];
        if id == IE_VENDOR && len >= 6 + 16 && value[..4] == [0x00, 0x0F, 0xAC, 0x01] {
            let key_id = value[4] & 3;
            let gtk: [u8; 16] = value[6..22].try_into().ok()?;
            return Some((key_id, gtk));
        }
        i += 2 + len;
    }
    None
}

pub fn ccmp_nonce_aad(header: &[u8], pn: u64) -> ([u8; 13], Vec<u8>) {
    let qos = (header[0] >> 2) & 3 == TYPE_DATA && header[0] & 0x80 != 0 && header.len() >= 26;
    let tid = if qos { header[24] & 0x0F } else { 0 };
    let mut nonce = [0u8; 13];
    nonce[0] = tid;
    nonce[1..7].copy_from_slice(&header[10..16]);
    let pn_be = pn.to_be_bytes();
    nonce[7..13].copy_from_slice(&pn_be[2..8]);
    let mut aad = Vec::with_capacity(24);
    aad.push(header[0] & 0x8F);
    aad.push((header[1] & 0xC7) | 0x40);
    aad.extend_from_slice(&header[4..22]);
    aad.push(header[22] & 0x0F);
    aad.push(0);
    if qos {
        aad.push(tid);
        aad.push(0);
    }
    (nonce, aad)
}
