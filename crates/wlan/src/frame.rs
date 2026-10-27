use alloc::string::String;
use alloc::vec::Vec;

pub const TYPE_MGMT: u8 = 0;
pub const TYPE_DATA: u8 = 2;

pub const SUB_ASSOC_REQ: u8 = 0;
pub const SUB_ASSOC_RESP: u8 = 1;
pub const SUB_PROBE_REQ: u8 = 4;
pub const SUB_PROBE_RESP: u8 = 5;
pub const SUB_BEACON: u8 = 8;
pub const SUB_DISASSOC: u8 = 10;
pub const SUB_AUTH: u8 = 11;
pub const SUB_DEAUTH: u8 = 12;

pub const FC_TO_DS: u8 = 0x01;
pub const FC_FROM_DS: u8 = 0x02;
pub const FC_PROTECTED: u8 = 0x40;

pub const BROADCAST: [u8; 6] = [0xFF; 6];

pub const IE_SSID: u8 = 0;
pub const IE_RATES: u8 = 1;
pub const IE_DS: u8 = 3;
pub const IE_RSN: u8 = 48;
pub const IE_EXT_RATES: u8 = 50;
pub const IE_VENDOR: u8 = 221;

pub const RATES: [u8; 8] = [0x82, 0x84, 0x8B, 0x96, 0x0C, 0x12, 0x18, 0x24];
pub const EXT_RATES: [u8; 4] = [0x30, 0x48, 0x60, 0x6C];

pub const RSN_IE_CCMP_PSK: [u8; 20] = [0x01, 0x00, 0x00, 0x0F, 0xAC, 0x04, 0x01, 0x00, 0x00, 0x0F, 0xAC, 0x04, 0x01, 0x00, 0x00, 0x0F, 0xAC, 0x02, 0x00, 0x00];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Security {
    Open,
    Wep,
    Wpa,
    Wpa2,
}

impl Security {
    pub fn name(&self) -> &'static str {
        match self {
            Security::Open => "open",
            Security::Wep => "wep",
            Security::Wpa => "wpa",
            Security::Wpa2 => "wpa2",
        }
    }
}

pub struct Header<'a> {
    pub frame_type: u8,
    pub subtype: u8,
    pub flags: u8,
    pub addr1: [u8; 6],
    pub addr2: [u8; 6],
    pub addr3: [u8; 6],
    pub sequence: u16,
    pub header_len: usize,
    pub body: &'a [u8],
}

pub fn parse(frame: &[u8]) -> Option<Header<'_>> {
    if frame.len() < 24 {
        return None;
    }
    let fc0 = frame[0];
    let flags = frame[1];
    let frame_type = (fc0 >> 2) & 3;
    let subtype = fc0 >> 4;
    let mut header_len = 24;
    if frame_type == TYPE_DATA {
        if flags & (FC_TO_DS | FC_FROM_DS) == (FC_TO_DS | FC_FROM_DS) {
            header_len += 6;
        }
        if subtype & 0x08 != 0 {
            header_len += 2;
        }
    }
    if frame.len() < header_len {
        return None;
    }
    let mac = |o: usize| -> [u8; 6] { frame[o..o + 6].try_into().unwrap() };
    Some(Header {
        frame_type,
        subtype,
        flags,
        addr1: mac(4),
        addr2: mac(10),
        addr3: mac(16),
        sequence: u16::from_le_bytes([frame[22], frame[23]]) >> 4,
        header_len,
        body: &frame[header_len..],
    })
}

pub fn build_header(out: &mut Vec<u8>, frame_type: u8, subtype: u8, flags: u8, addr1: [u8; 6], addr2: [u8; 6], addr3: [u8; 6], sequence: u16) {
    out.push((subtype << 4) | (frame_type << 2));
    out.push(flags);
    out.extend_from_slice(&[0x3A, 0x01]);
    out.extend_from_slice(&addr1);
    out.extend_from_slice(&addr2);
    out.extend_from_slice(&addr3);
    out.extend_from_slice(&((sequence & 0x0FFF) << 4).to_le_bytes());
}

pub struct Elements<'a> {
    data: &'a [u8],
}

impl<'a> Iterator for Elements<'a> {
    type Item = (u8, &'a [u8]);

    fn next(&mut self) -> Option<Self::Item> {
        if self.data.len() < 2 {
            return None;
        }
        let id = self.data[0];
        let len = self.data[1] as usize;
        if self.data.len() < 2 + len {
            return None;
        }
        let value = &self.data[2..2 + len];
        self.data = &self.data[2 + len..];
        Some((id, value))
    }
}

pub fn elements(data: &[u8]) -> Elements<'_> {
    Elements { data }
}

pub struct BeaconInfo {
    pub ssid: String,
    pub channel: u8,
    pub security: Security,
    pub rsn_ie: Vec<u8>,
}

pub fn parse_beacon(body: &[u8]) -> Option<BeaconInfo> {
    if body.len() < 12 {
        return None;
    }
    let capability = u16::from_le_bytes([body[10], body[11]]);
    let mut info = BeaconInfo { ssid: String::new(), channel: 0, security: if capability & 0x10 != 0 { Security::Wep } else { Security::Open }, rsn_ie: Vec::new() };
    for (id, value) in elements(&body[12..]) {
        match id {
            IE_SSID => info.ssid = String::from_utf8_lossy(value).into_owned(),
            IE_DS if !value.is_empty() => info.channel = value[0],
            IE_RSN => {
                info.security = Security::Wpa2;
                info.rsn_ie = value.to_vec();
            }
            IE_VENDOR if value.len() >= 4 && value[..4] == [0x00, 0x50, 0xF2, 0x01] && info.security != Security::Wpa2 => info.security = Security::Wpa,
            _ => {}
        }
    }
    Some(info)
}

pub fn rsn_supports_ccmp_psk(rsn: &[u8]) -> bool {
    if rsn.len() < 8 {
        return false;
    }
    let mut offset = 6;
    let pairwise = u16::from_le_bytes([rsn[offset], rsn[offset + 1]]) as usize;
    offset += 2;
    let mut ccmp = false;
    for i in 0..pairwise {
        let o = offset + i * 4;
        if rsn.len() >= o + 4 && rsn[o..o + 4] == [0x00, 0x0F, 0xAC, 0x04] {
            ccmp = true;
        }
    }
    offset += pairwise * 4;
    if rsn.len() < offset + 2 {
        return ccmp;
    }
    let akm = u16::from_le_bytes([rsn[offset], rsn[offset + 1]]) as usize;
    offset += 2;
    let mut psk = false;
    for i in 0..akm {
        let o = offset + i * 4;
        if rsn.len() >= o + 4 && rsn[o..o + 4] == [0x00, 0x0F, 0xAC, 0x02] {
            psk = true;
        }
    }
    ccmp && psk
}

pub fn channel_frequency(channel: u8) -> u32 {
    match channel {
        14 => 2484,
        1..=13 => 2407 + channel as u32 * 5,
        _ => 5000 + channel as u32 * 5,
    }
}
