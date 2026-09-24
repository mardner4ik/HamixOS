use hamix_kpi::{self as kpi, ConnectorDesc};

use crate::aux;
use crate::edid;
use crate::mode::Output;
use crate::regs::{Gen, Mmio};

pub const MAX: usize = 4;

const SFUSE_STRAP: u32 = 0xC2014;
const SDEISR: u32 = 0xC4000;
const DDI_BUF_CTL: u32 = 0x64000;
const TRANS_DDI_FUNC_CTL: [u32; 3] = [0x60400, 0x61400, 0x62400];
const PCH_LVDS: u32 = 0xE1180;
const CPU_DP_A: u32 = 0x64000;
const ENABLE: u32 = 1 << 31;

#[derive(Clone, Copy)]
pub struct Connector {
    pub desc: ConnectorDesc,
    pub edid: [u8; 256],
    pub edid_len: usize,
    pub active: bool,
}

impl Connector {
    fn new(kind: u32, id: u32, name: &str) -> Connector {
        Connector { desc: ConnectorDesc::named(kind, id, name), edid: [0; 256], edid_len: 0, active: false }
    }
}

fn gmbus_pin(port: u32) -> u32 {
    match port {
        1 => 5,
        2 => 4,
        _ => 6,
    }
}

fn active_ddi(mmio: &Mmio, output: &Output) -> (u32, u32) {
    if output.transcoder >= 3 {
        return (0, kpi::CONNECTOR_EDP);
    }
    let func = mmio.read(TRANS_DDI_FUNC_CTL[output.transcoder.min(2)]);
    let port = (func >> 28) & 0x7;
    let kind = match (func >> 24) & 0x7 {
        0 => kpi::CONNECTOR_HDMI,
        1 => kpi::CONNECTOR_DVI,
        2 | 3 => kpi::CONNECTOR_DP,
        _ => kpi::CONNECTOR_UNKNOWN,
    };
    (port, kind)
}

fn name_text(name: &[u8; 8]) -> &str {
    let end = name.iter().position(|b| *b == 0).unwrap_or(name.len());
    unsafe { core::str::from_utf8_unchecked(&name[..end]) }
}

fn name_for(kind: u32, port: u32) -> [u8; 8] {
    let prefix: &[u8] = match kind {
        kpi::CONNECTOR_EDP => b"eDP-",
        kpi::CONNECTOR_LVDS => b"LVDS-",
        kpi::CONNECTOR_HDMI => b"HDMI-",
        kpi::CONNECTOR_DVI => b"DVI-",
        kpi::CONNECTOR_DP => b"DP-",
        _ => b"DDI-",
    };
    let mut out = [0u8; 8];
    out[..prefix.len()].copy_from_slice(prefix);
    out[prefix.len()] = if kind == kpi::CONNECTOR_EDP || kind == kpi::CONNECTOR_LVDS { b'1' } else { b'A' + port as u8 };
    out
}

fn fill_edid(mmio: &Mmio, chip: Gen, connector: &mut Connector, port: u32, dp: bool, allow: bool) {
    if !allow {
        return;
    }
    connector.edid_len = if dp { aux::read_edid(mmio, chip, port, &mut connector.edid) } else { edid::read_pin(mmio, gmbus_pin(port), &mut connector.edid) };
    connector.desc.edid_len = connector.edid_len as u32;
    if connector.edid_len >= 128 {
        let mut modes = [0u32; 3];
        if kpi::edid_modes(&connector.edid[..connector.edid_len], &mut modes) > 0 {
            connector.desc.native_w = modes[0];
            connector.desc.native_h = modes[1];
            connector.desc.refresh = modes[2];
        }
    }
}

pub fn detect(mmio: &Mmio, chip: Gen, output: &Output, allow_edid: bool) -> ([Connector; MAX], usize) {
    let mut list = [Connector::new(0, 0, ""); MAX];
    let mut count = 0usize;
    let ddi = matches!(chip, Gen::Gen7_5 | Gen::Gen8 | Gen::Gen9);
    let (active_port, active_kind) = if ddi {
        active_ddi(mmio, output)
    } else if mmio.read(PCH_LVDS) & ENABLE != 0 {
        (0, kpi::CONNECTOR_LVDS)
    } else if mmio.read(CPU_DP_A) & ENABLE != 0 {
        (0, kpi::CONNECTOR_EDP)
    } else {
        (1, kpi::CONNECTOR_HDMI)
    };
    let name = name_for(active_kind, active_port);
    let mut primary = Connector::new(active_kind, 0, name_text(&name));
    primary.active = true;
    primary.desc.status = 1;
    primary.desc.primary = 1;
    primary.desc.native_w = output.native.hactive;
    primary.desc.native_h = output.native.vactive;
    primary.desc.refresh = (output.refresh_mhz + 500) / 1000;
    let dp = ddi && (active_kind == kpi::CONNECTOR_EDP || active_kind == kpi::CONNECTOR_DP);
    fill_edid(mmio, chip, &mut primary, active_port, dp, allow_edid);
    primary.desc.native_w = output.native.hactive;
    primary.desc.native_h = output.native.vactive;
    list[0] = primary;
    count += 1;
    if !ddi {
        return (list, count);
    }
    let strap = mmio.read(SFUSE_STRAP);
    let live = mmio.read(SDEISR);
    for port in 1..=3u32 {
        if count >= MAX || port == active_port {
            continue;
        }
        let present = strap & (1 << (3 - port)) != 0 || mmio.read(DDI_BUF_CTL + port * 0x100) & ENABLE != 0;
        if !present {
            continue;
        }
        let connected = live & (1 << (20 + port)) != 0;
        let name = name_for(kpi::CONNECTOR_DP, port);
        let mut connector = Connector::new(kpi::CONNECTOR_DP, count as u32, name_text(&name));
        connector.desc.status = connected as u32;
        if connected {
            fill_edid(mmio, chip, &mut connector, port, true, allow_edid);
            if connector.edid_len == 0 {
                connector.desc.kind = kpi::CONNECTOR_HDMI;
                fill_edid(mmio, chip, &mut connector, port, false, allow_edid);
            }
        }
        list[count] = connector;
        count += 1;
    }
    (list, count)
}
