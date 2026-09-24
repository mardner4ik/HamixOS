#![no_std]

extern crate alloc;

mod ini;

use alloc::collections::VecDeque;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::ffi::c_void;

use hamix_kpi::io::{DmaRegion, Mmio};
use hamix_kpi::{self as kpi, PciHandle, WifiBss, WifiNetOps, WifiOps, WifiStatus};
use wlan::{Radio, Station};

const AR_CR: u32 = 0x0008;
const AR_RXDP: u32 = 0x000C;
const AR_CFG: u32 = 0x0014;
const AR_MIRT: u32 = 0x0020;
const AR_TXCFG: u32 = 0x0030;
const AR_RXCFG: u32 = 0x0034;
const AR_GTXTO: u32 = 0x0064;
const AR_GTTM: u32 = 0x0068;
const AR_CST: u32 = 0x006C;
const AR_ISR: u32 = 0x0080;
const AR_IMR: u32 = 0x00A0;
const AR_Q0_TXDP: u32 = 0x0800;
const AR_Q_TXE: u32 = 0x0840;
const AR_Q_TXD: u32 = 0x0880;
const AR_Q0_MISC: u32 = 0x09C0;
const AR_D0_QCUMASK: u32 = 0x1000;
const AR_D0_LCL_IFS: u32 = 0x1040;
const AR_D0_RETRY_LIMIT: u32 = 0x1080;
const AR_D0_MISC: u32 = 0x1100;
const AR_MAC_LED: u32 = 0x1F04;
const AR_RC: u32 = 0x4000;
const AR_SREV: u32 = 0x4020;
const AR_AHB_MODE: u32 = 0x4024;
const AR_INTR_SYNC_CAUSE: u32 = 0x4028;
const AR_INTR_SYNC_ENABLE: u32 = 0x402C;
const AR_EEPROM_STATUS_DATA: u32 = 0x407C;
const AR_OBS: u32 = 0x4080;
const AR_RTC_BASE: u32 = 0x7000;
const AR_RTC_RC: u32 = AR_RTC_BASE;
const AR_RTC_PLL_CONTROL: u32 = AR_RTC_BASE + 0x14;
const AR_RTC_RESET: u32 = AR_RTC_BASE + 0x40;
const AR_RTC_STATUS: u32 = AR_RTC_BASE + 0x44;
const AR_RTC_SLEEP_CLK: u32 = AR_RTC_BASE + 0x48;
const AR_RTC_FORCE_WAKE: u32 = AR_RTC_BASE + 0x4C;
const AR9285_AN_RF2G3: u32 = 0x7828;
const AR9285_AN_RF2G4: u32 = 0x782C;
const AR_STA_ID0: u32 = 0x8000;
const AR_STA_ID1: u32 = 0x8004;
const AR_BSS_ID0: u32 = 0x8008;
const AR_BSS_ID1: u32 = 0x800C;
const AR_RSSI_THR: u32 = 0x8018;
const AR_RX_FILTER: u32 = 0x803C;
const AR_DIAG_SW: u32 = 0x8048;
const AR_DIAG_ENCRYPT_DIS: u32 = 0x08;
const AR_DIAG_DECRYPT_DIS: u32 = 0x10;
const AR_KEYTABLE_0: u32 = 0x8800;
const AR_KEYTABLE_TYPE_CLR: u32 = 7;
const KEY_CACHE_ENTRIES: u32 = 128;
const AR_BSSMSKL: u32 = 0x80E0;
const AR_BSSMSKU: u32 = 0x80E4;
const AR_TPC: u32 = 0x80E8;
const AR_NOACK: u32 = 0x8108;
const AR_RXFIFO_CFG: u32 = 0x8114;
const AR_QOS_CONTROL: u32 = 0x8118;
const AR_QOS_SELECT: u32 = 0x811C;
const AR_TXOP_X: u32 = 0x81EC;
const AR_TXOP_0_3: u32 = 0x81F0;
const AR_SELFGEN_MASK: u32 = 0x832C;
const AR_PCU_TXBUF_CTRL: u32 = 0x8340;
const AR_PCU_MISC_MODE2: u32 = 0x8344;
const AR_PHY_BASE: u32 = 0x9800;
const AR_PHY_TURBO: u32 = 0x9804;
const AR_PHY_ACTIVE: u32 = 0x981C;
const AR_PHY_RF_CTL3: u32 = 0x9828;
const AR_PHY_ADC_SERIAL_CTL: u32 = 0x9830;
const AR_PHY_RF_CTL4: u32 = 0x9834;
const AR_PHY_SETTLING: u32 = 0x9844;
const AR_PHY_DESIRED_SZ: u32 = 0x9850;
const AR_PHY_AGC_CONTROL: u32 = 0x9860;
const AR_PHY_CCA: u32 = 0x9864;
const AR_PHY_SYNTH_CONTROL: u32 = 0x9874;
const AR_PHY_RX_DELAY: u32 = 0x9914;
const AR_PHY_POWER_TX_RATE1: u32 = 0x9934;
const AR_PHY_POWER_TX_RATE2: u32 = 0x9938;
const AR_PHY_POWER_TX_RATE_MAX: u32 = 0x993C;
const AR_PHY_SWITCH_COM: u32 = 0x9964;
const AR_PHY_RX_CHAINMASK: u32 = 0x99A4;
const AR_PHY_EXT_CCA0: u32 = 0x99B8;
const AR_PHY_MODE: u32 = 0xA200;
const AR_PHY_CCK_TX_CTRL: u32 = 0xA204;
const AR_PHY_POWER_TX_RATE3: u32 = 0xA234;
const AR_PHY_POWER_TX_RATE4: u32 = 0xA238;
const AR_PHY_POWER_TX_RATE5: u32 = 0xA38C;
const AR_PHY_POWER_TX_RATE6: u32 = 0xA390;
const AR_PHY_CAL_CHAINMASK: u32 = 0xA39C;
const AR_PHY_POWER_TX_RATE7: u32 = 0xA3CC;
const AR_PHY_POWER_TX_RATE8: u32 = 0xA3D0;
const AR_PHY_POWER_TX_RATE9: u32 = 0xA3D4;

const EEPROM_OFFSET: u32 = 0x2000;
const EEPROM_MAGIC: u16 = 0xA55A;
const EEPROM_4K_START: u32 = 64;
const EEPROM_4K_WORDS: usize = 0x1E0;

const RX_COUNT: usize = 64;
const RX_BUFFER: usize = 3872;
const TX_COUNT: usize = 32;
const TX_BUFFER: usize = 2400;
const DESC_SIZE: usize = 128;

const RATE_1M: u32 = 0x1B;
const RATE_2M: u32 = 0x1A;
const RATE_11M: u32 = 0x18;

const AR_TSF_L32: u32 = 0x804C;

const IDS: &[(u16, &str)] = &[(0x002B, "Qualcomm Atheros AR9285")];

struct Hw {
    io: Mmio,
    mac: [u8; 6],
    channel: u8,
    rx_ring: DmaRegion,
    rx_buffers: DmaRegion,
    rx_next: usize,
    tx_ring: DmaRegion,
    tx_buffers: DmaRegion,
    tx_head: usize,
    tx_tail: usize,
    tx_busy: bool,
    tx_queue: VecDeque<(Vec<u8>, bool)>,
    rx_frames: u64,
    tx_frames: u64,
    tx_failed: u64,
    running: bool,
}

struct Ar9285 {
    revision: u32,
    eeprom_version: u16,
    hw: Hw,
    station: Station,
    enabled: bool,
    last_tick: u64,
    status: String,
    id: i32,
    carrier: bool,
}

static mut NIC: Option<Ar9285> = None;

fn nic() -> Option<&'static mut Ar9285> {
    unsafe { (&mut *(&raw mut NIC)).as_mut() }
}

fn delay_us(us: u64) {
    kpi::io::delay_us(us);
}

impl Hw {
    fn read(&self, reg: u32) -> u32 {
        self.io.read32(reg)
    }

    fn write(&self, reg: u32, value: u32) {
        self.io.write32(reg, value)
    }

    fn rmw(&self, reg: u32, mask: u32, shift: u32, value: u32) {
        let v = (self.read(reg) & !mask) | ((value << shift) & mask);
        self.write(reg, v);
    }

    fn set_bits(&self, reg: u32, bits: u32) {
        self.write(reg, self.read(reg) | bits);
    }

    fn clear_bits(&self, reg: u32, bits: u32) {
        self.write(reg, self.read(reg) & !bits);
    }

    fn wait(&self, reg: u32, mask: u32, value: u32, tries: u32) -> bool {
        for _ in 0..tries {
            if self.read(reg) & mask == value {
                return true;
            }
            delay_us(10);
        }
        false
    }

    fn eeprom_read(&self, offset: u32) -> Option<u16> {
        let _ = self.read(EEPROM_OFFSET + (offset << 2));
        if !self.wait(AR_EEPROM_STATUS_DATA, 0x0001_0000 | 0x0004_0000, 0, 1000) {
            return None;
        }
        Some((self.read(AR_EEPROM_STATUS_DATA) & 0xFFFF) as u16)
    }

    fn power_on_reset(&self) -> bool {
        self.write(AR_RTC_FORCE_WAKE, 0x3);
        self.write(AR_RC, 0x1);
        self.write(AR_RTC_RESET, 0);
        delay_us(20);
        self.write(AR_RC, 0);
        self.write(AR_RTC_RESET, 1);
        if !self.wait(AR_RTC_STATUS, 0xF, 0x2, 5000) {
            return false;
        }
        self.mac_reset(true)
    }

    fn mac_reset(&self, cold: bool) -> bool {
        self.write(AR_RTC_FORCE_WAKE, 0x3);
        let cause = self.read(AR_INTR_SYNC_CAUSE);
        if cause & (0x2000 | 0x1000) != 0 {
            self.write(AR_INTR_SYNC_ENABLE, 0);
            self.write(AR_RC, 0x1 | 0x100);
        } else {
            self.write(AR_RC, 0x1);
        }
        self.write(AR_RTC_RC, 0x1 | if cold { 0x2 } else { 0 });
        delay_us(100);
        self.write(AR_RTC_RC, 0);
        if !self.wait(AR_RTC_RC, 0x3, 0, 5000) {
            return false;
        }
        self.write(AR_RC, 0);
        if cold {
            self.write(AR_CFG, 0);
        }
        true
    }

    fn wake(&self) -> bool {
        self.set_bits(AR_RTC_FORCE_WAKE, 0x1);
        if self.read(AR_RTC_STATUS) & 0xF != 0x2 {
            if !self.power_on_reset() {
                return false;
            }
        }
        self.wait(AR_RTC_STATUS, 0xF, 0x2, 5000)
    }

    fn init_pll(&self) {
        self.write(AR_RTC_PLL_CONTROL, (0x5 << 10) | 0x2C);
        delay_us(1000);
        self.write(AR_RTC_SLEEP_CLK, 0x2);
    }

    fn write_table(&self, table: &[(u32, u32)]) {
        for (i, (reg, value)) in table.iter().enumerate() {
            self.write(*reg, *value);
            if (0x7800..0x7900).contains(reg) {
                delay_us(100);
            }
            if i % 32 == 31 {
                core::hint::spin_loop();
            }
        }
    }

    fn set_channel(&mut self, channel: u8) -> bool {
        if !(1..=14).contains(&channel) {
            return false;
        }
        let freq = wlan::frame::channel_frequency(channel);
        let mut reg = self.read(AR_PHY_SYNTH_CONTROL) & 0xC000_0000;
        let channel_sel = (freq * 0x10000) / 15;
        let tx_ctl = self.read(AR_PHY_CCK_TX_CTRL);
        if freq == 2484 {
            self.write(AR_PHY_CCK_TX_CTRL, tx_ctl | 0x10);
        } else {
            self.write(AR_PHY_CCK_TX_CTRL, tx_ctl & !0x10);
        }
        reg |= (1 << 29) | (1 << 28) | channel_sel;
        self.write(AR_PHY_SYNTH_CONTROL, reg);
        self.channel = channel;
        delay_us(2000);
        true
    }

    fn setup_rx(&mut self) {
        for i in 0..RX_COUNT {
            let desc = self.rx_ring.ptr::<u32>(i * DESC_SIZE);
            let next = self.rx_ring.phys as usize + ((i + 1) % RX_COUNT) * DESC_SIZE;
            unsafe {
                core::ptr::write_bytes(desc as *mut u8, 0, DESC_SIZE);
                *desc = next as u32;
                *desc.add(1) = (self.rx_buffers.phys + (i * RX_BUFFER) as u64) as u32;
                *desc.add(3) = RX_BUFFER as u32 & 0xFFF;
            }
        }
        self.rx_next = 0;
        self.write(AR_RXDP, self.rx_ring.phys as u32);
        self.write(AR_CR, 0x4);
        self.clear_bits(AR_DIAG_SW, 0x20 | 0x0200_0000);
    }

    fn clear_key_cache(&self) {
        for entry in 0..KEY_CACHE_ENTRIES {
            let base = AR_KEYTABLE_0 + entry * 32;
            for word in 0..5 {
                self.write(base + word * 4, 0);
            }
            self.write(base + 20, AR_KEYTABLE_TYPE_CLR);
            self.write(base + 24, 0);
            self.write(base + 28, 0);
        }
    }

    fn set_filter(&self, associated: bool) {
        let mut filter = 0x1 | 0x2 | 0x4 | 0x10;
        if !associated {
            filter |= 0x80;
        }
        self.write(AR_RX_FILTER, filter);
    }

    fn set_bssid_hw(&self, bssid: Option<[u8; 6]>) {
        let b = bssid.unwrap_or([0; 6]);
        self.write(AR_BSS_ID0, u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
        self.write(AR_BSS_ID1, u16::from_le_bytes([b[4], b[5]]) as u32);
        self.set_filter(bssid.is_some());
    }

    fn collect_rx(&mut self, out: &mut Vec<(Vec<u8>, i8)>) {
        for _ in 0..RX_COUNT {
            let desc = self.rx_ring.ptr::<u32>(self.rx_next * DESC_SIZE);
            let status8 = unsafe { *desc.add(12) };
            if status8 & 0x1 == 0 {
                break;
            }
            let status1 = unsafe { *desc.add(5) };
            let status4 = unsafe { *desc.add(8) };
            let len = (status1 & 0xFFF) as usize;
            let ok = status8 & (0x2 | 0x8) != 0 && status8 & (0x4 | 0x10) == 0 && len > 4 && len <= RX_BUFFER;
            if ok {
                let rssi_raw = ((status4 >> 24) & 0xFF) as i32;
                let rssi = if rssi_raw >= 128 { -95 } else { (rssi_raw - 95).clamp(-100, 0) };
                let frame = self.rx_buffers.slice(self.rx_next * RX_BUFFER, len - 4).to_vec();
                out.push((frame, rssi as i8));
                self.rx_frames += 1;
            }
            unsafe {
                for w in 4..13 {
                    *desc.add(w) = 0;
                }
                *desc.add(3) = RX_BUFFER as u32 & 0xFFF;
            }
            self.rx_next = (self.rx_next + 1) % RX_COUNT;
        }
        if self.read(AR_CR) & 0x4 == 0 && self.running {
            self.write(AR_CR, 0x4);
        }
    }

    fn start_tx(&mut self, frame: &[u8], management: bool) -> bool {
        if frame.len() + 4 > TX_BUFFER || frame.len() < 10 {
            return false;
        }
        let slot = self.tx_head;
        let desc = self.tx_ring.ptr::<u32>(slot * DESC_SIZE);
        self.tx_buffers.slice(slot * TX_BUFFER, frame.len()).copy_from_slice(frame);
        let broadcast = frame[4] & 1 != 0;
        let power: u32 = 30;
        let (rate0, rate1) = if management || broadcast { (RATE_1M, RATE_1M) } else { (RATE_11M, RATE_2M) };
        let tries0 = if broadcast { 1 } else { 4 };
        unsafe {
            core::ptr::write_bytes(desc as *mut u8, 0, DESC_SIZE);
            *desc = 0;
            *desc.add(1) = (self.tx_buffers.phys + (slot * TX_BUFFER) as u64) as u32;
            *desc.add(2) = ((frame.len() + 4) as u32 & 0xFFF) | (power << 16) | 0x0100_0000;
            *desc.add(3) = (frame.len() as u32 & 0xFFF) | if broadcast { 0x0100_0000 } else { 0 };
            *desc.add(4) = (tries0 << 16) | if broadcast { 0 } else { 4 << 20 };
            *desc.add(5) = rate0 | (rate1 << 8);
            *desc.add(9) = (1 << 2) | (1 << 7) | (1 << 12) | (1 << 17);
        }
        self.write(AR_Q0_TXDP, self.tx_ring.phys as u32 + (slot * DESC_SIZE) as u32);
        self.write(AR_Q_TXE, 1);
        self.tx_busy = true;
        self.tx_tail = slot;
        self.tx_head = (self.tx_head + 1) % TX_COUNT;
        true
    }

    fn service_tx(&mut self) {
        if self.tx_busy {
            let desc = self.tx_ring.ptr::<u32>(self.tx_tail * DESC_SIZE);
            let status9 = unsafe { *desc.add(23) };
            let status1 = unsafe { *desc.add(15) };
            if status9 & 0x1 == 0 && self.read(AR_Q_TXE) & 1 != 0 {
                return;
            }
            self.tx_busy = false;
            if status1 & 0x1 != 0 {
                self.tx_frames += 1;
            } else {
                self.tx_failed += 1;
            }
        }
        if let Some((frame, management)) = self.tx_queue.pop_front() {
            self.start_tx(&frame, management);
        }
    }
}

impl Radio for Hw {
    fn mac(&self) -> [u8; 6] {
        self.mac
    }

    fn set_channel(&mut self, channel: u8) -> bool {
        Hw::set_channel(self, channel)
    }

    fn set_bssid(&mut self, bssid: Option<[u8; 6]>) {
        self.set_bssid_hw(bssid);
    }

    fn transmit(&mut self, frame: &[u8], management: bool) -> bool {
        if !self.running {
            return false;
        }
        if self.tx_busy || !self.tx_queue.is_empty() {
            if self.tx_queue.len() >= 64 {
                return false;
            }
            self.tx_queue.push_back((frame.to_vec(), management));
            return true;
        }
        self.start_tx(frame, management)
    }
}

impl Ar9285 {
    fn board_values(&self, eeprom: &[u16]) {
        let byte = |offset: usize| -> u8 {
            let word = eeprom.get(offset / 2).copied().unwrap_or(0);
            if offset % 2 == 0 { word as u8 } else { (word >> 8) as u8 }
        };
        let modal = 52usize;
        let u32_at = |offset: usize| -> u32 { byte(offset) as u32 | (byte(offset + 1) as u32) << 8 | (byte(offset + 2) as u32) << 16 | (byte(offset + 3) as u32) << 24 };
        let hw = &self.hw;
        hw.write(AR_PHY_SWITCH_COM, u32_at(modal + 4));
        let version = byte(modal + 37);
        let nib = |offset: usize, high: bool| -> u8 { if high { byte(offset) >> 4 } else { byte(offset) & 0xF } };
        let (mut ob, mut db1, mut db2) = ([0u8; 5], [0u8; 5], [0u8; 5]);
        ob[0] = nib(modal + 25, false);
        ob[1] = nib(modal + 25, true);
        db1[0] = nib(modal + 26, false);
        db1[1] = nib(modal + 26, true);
        db2[0] = nib(modal + 36, false);
        db2[1] = nib(modal + 36, true);
        if version >= 2 {
            ob[2] = nib(modal + 38, false);
            ob[3] = nib(modal + 38, true);
            ob[4] = nib(modal + 39, false);
            db1[2] = nib(modal + 40, false);
            db1[3] = nib(modal + 40, true);
            db1[4] = nib(modal + 41, false);
            db2[2] = nib(modal + 42, false);
            db2[3] = nib(modal + 42, true);
            db2[4] = nib(modal + 43, false);
        } else {
            for i in 2..5 {
                ob[i] = ob[1];
                db1[i] = db1[1];
                db2[i] = db2[1];
            }
        }
        let ob_fields = [(0x00E0_0000, 21), (0x001C_0000, 18), (0x0003_8000, 15), (0x0000_7000, 12), (0x0000_0E00, 9)];
        for (i, (mask, shift)) in ob_fields.iter().enumerate() {
            hw.rmw(AR9285_AN_RF2G3, *mask, *shift, ob[i] as u32);
            delay_us(100);
        }
        let db1_fields = [(AR9285_AN_RF2G3, 0x1C0, 6), (AR9285_AN_RF2G3, 0x38, 3), (AR9285_AN_RF2G3, 0x7, 0), (AR9285_AN_RF2G4, 0xE000_0000, 29), (AR9285_AN_RF2G4, 0x1C00_0000, 26)];
        for (i, (reg, mask, shift)) in db1_fields.iter().enumerate() {
            hw.rmw(*reg, *mask, *shift, db1[i] as u32);
            delay_us(100);
        }
        let db2_fields = [(0x0380_0000, 23), (0x0070_0000, 20), (0x000E_0000, 17), (0x0001_C000, 14), (0x0000_3800, 11)];
        for (i, (mask, shift)) in db2_fields.iter().enumerate() {
            hw.rmw(AR9285_AN_RF2G4, *mask, *shift, db2[i] as u32);
            delay_us(100);
        }
        hw.rmw(AR_PHY_SETTLING, 0x3F80, 7, byte(modal + 9) as u32);
        hw.rmw(AR_PHY_DESIRED_SZ, 0xFF, 0, byte(modal + 12) as u32);
        let tx_end_off = byte(modal + 15) as u32;
        let tx_frame_on = byte(modal + 17) as u32;
        hw.write(AR_PHY_RF_CTL4, (tx_end_off << 16) | (tx_end_off << 24) | tx_frame_on | (tx_frame_on << 8));
        hw.rmw(AR_PHY_RF_CTL3, 0x00FF_0000, 16, byte(modal + 16) as u32);
        let thresh62 = byte(modal + 18) as u32;
        hw.rmw(AR_PHY_CCA, 0x000F_F000, 12, thresh62);
        hw.rmw(AR_PHY_EXT_CCA0, 0xFF, 0, thresh62);
    }

    fn write_power(&self) {
        let level: u32 = 30;
        let four = level | (level << 8) | (level << 16) | (level << 24);
        for reg in [AR_PHY_POWER_TX_RATE1, AR_PHY_POWER_TX_RATE2, AR_PHY_POWER_TX_RATE3, AR_PHY_POWER_TX_RATE4, AR_PHY_POWER_TX_RATE5, AR_PHY_POWER_TX_RATE6, AR_PHY_POWER_TX_RATE7, AR_PHY_POWER_TX_RATE8, AR_PHY_POWER_TX_RATE9] {
            self.hw.write(reg, four);
        }
        self.hw.write(AR_PHY_POWER_TX_RATE_MAX, 63);
    }

    fn reset(&mut self, eeprom: &[u16], tx_gain_high: bool) -> Result<(), &'static str> {
        if !self.hw.wake() {
            return Err("the chip did not wake up");
        }
        let saved_led = self.hw.read(AR_MAC_LED) & (0x1 | 0x3 << 2 | 0x3 << 5 | 0x1 << 7);
        if !self.hw.mac_reset(false) {
            return Err("MAC reset timed out");
        }
        if !self.hw.wake() {
            return Err("the chip fell asleep after reset");
        }
        self.hw.init_pll();
        self.hw.write(AR_PHY_MODE, 0x4);
        self.hw.write(AR_PHY_BASE, 0x7);
        self.hw.write(AR_PHY_ADC_SERIAL_CTL, 0);
        self.hw.write_table(ini::MODES_2G_HT20);
        self.hw.write_table(if tx_gain_high { ini::TX_GAIN_HIGH_POWER } else { ini::TX_GAIN_ORIGINAL });
        self.hw.write_table(ini::COMMON);
        self.hw.set_bits(AR_DIAG_SW, 0x20 | 0x0200_0000 | AR_DIAG_ENCRYPT_DIS | AR_DIAG_DECRYPT_DIS);
        self.hw.clear_key_cache();
        let misc2 = self.hw.read(AR_PCU_MISC_MODE2) & !(0x40 | 0x0010_0000);
        self.hw.write(AR_PCU_MISC_MODE2, misc2);
        let dac_fifo = self.hw.read(AR_PHY_TURBO) & 0x800;
        self.hw.write(AR_PHY_TURBO, 0x40 | 0x80 | 0x200 | 0x100 | dac_fifo);
        self.hw.write(AR_GTXTO, 25 << 16);
        self.hw.set_bits(AR_GTTM, 0x8);
        self.hw.write(AR_CST, 0xF << 16);
        self.hw.write(AR_PHY_RX_CHAINMASK, 1);
        self.hw.write(AR_PHY_CAL_CHAINMASK, 1);
        self.hw.write(AR_SELFGEN_MASK, 1);
        self.write_power();
        self.board_values(eeprom);
        let mac = self.hw.mac;
        self.hw.write(AR_STA_ID0, u32::from_le_bytes([mac[0], mac[1], mac[2], mac[3]]));
        self.hw.write(AR_STA_ID1, u16::from_le_bytes([mac[4], mac[5]]) as u32 | 0x0080_0000 | 0x2000_0000);
        self.hw.write(AR_BSSMSKL, 0xFFFF_FFFF);
        self.hw.write(AR_BSSMSKU, 0xFFFF);
        self.hw.write(AR_MAC_LED, self.hw.read(AR_MAC_LED) | saved_led);
        self.hw.set_bssid_hw(None);
        self.hw.write(AR_RSSI_THR, 0x0000_0700 | 20);
        self.hw.write(AR_ISR, 0xFFFF_FFFF);
        let channel = if self.hw.channel == 0 { 1 } else { self.hw.channel };
        self.hw.set_channel(channel);
        for i in 0..10 {
            self.hw.write(AR_D0_QCUMASK + (i << 2), 1 << i);
        }
        self.hw.write(AR_D0_LCL_IFS, 15 | (1023 << 10) | (2 << 20));
        self.hw.write(AR_D0_RETRY_LIMIT, (32 << 8) | (32 << 14) | (10 << 4) | 10);
        self.hw.write(AR_Q0_MISC, 0x800);
        self.hw.write(AR_D0_MISC, 0x100 | 0x2);
        self.hw.write(AR_IMR, 0x1 | 0x4 | 0x40 | 0x100);
        self.hw.write(AR_QOS_CONTROL, 0x100AA);
        self.hw.write(AR_QOS_SELECT, 0x3210);
        self.hw.write(AR_NOACK, 2 | (5 << 3));
        self.hw.write(AR_TXOP_X, 0xFF);
        for i in 0..4 {
            self.hw.write(AR_TXOP_0_3 + i * 4, 0xFFFF_FFFF);
        }
        self.hw.set_bits(AR_AHB_MODE, 0x4);
        let txcfg = (self.hw.read(AR_TXCFG) & !0x3) | 5;
        self.hw.write(AR_TXCFG, (txcfg & !0x3F0) | (0x3F << 4));
        let rxcfg = (self.hw.read(AR_RXCFG) & !0x7) | 5;
        self.hw.write(AR_RXCFG, rxcfg);
        self.hw.write(AR_RXFIFO_CFG, 0x200);
        self.hw.write(AR_PCU_TXBUF_CTRL, 0x380);
        self.hw.write(AR_OBS, 8);
        self.hw.write(AR_MIRT, 0);
        let synth_delay = (self.hw.read(AR_PHY_RX_DELAY) & 0x3FFF) * 4 / 22;
        self.hw.write(AR_PHY_ACTIVE, 1);
        delay_us(synth_delay as u64 * 100 + 100);
        self.hw.write(AR_TPC, 63 | (63 << 8) | (63 << 16));
        self.hw.set_bits(AR_PHY_AGC_CONTROL, 0x1);
        if !self.hw.wait(AR_PHY_AGC_CONTROL, 0x1, 0, 100_000) {
            kpi::dev_warn("initial calibration did not finish");
        }
        self.hw.set_bits(AR_PHY_AGC_CONTROL, 0x2);
        self.hw.running = true;
        self.hw.setup_rx();
        self.hw.write(AR_Q_TXD, 0);
        Ok(())
    }

    fn start(handle: &PciHandle, model: &'static str) -> Option<Ar9285> {
        let (base, len) = kpi::bar(handle, 0);
        let io = Mmio::map(base, len)?;
        unsafe { kpi::hamix_pci_enable(handle) };
        let hw = Hw {
            io,
            mac: [0; 6],
            channel: 1,
            rx_ring: DmaRegion::new(RX_COUNT * DESC_SIZE)?,
            rx_buffers: DmaRegion::new(RX_COUNT * RX_BUFFER)?,
            rx_next: 0,
            tx_ring: DmaRegion::new(TX_COUNT * DESC_SIZE)?,
            tx_buffers: DmaRegion::new(TX_COUNT * TX_BUFFER)?,
            tx_head: 0,
            tx_tail: 0,
            tx_busy: false,
            tx_queue: VecDeque::new(),
            rx_frames: 0,
            tx_frames: 0,
            tx_failed: 0,
            running: false,
        };
        if !hw.power_on_reset() || !hw.wake() {
            kpi::dev_err("power-on reset failed");
            return None;
        }
        let srev = hw.read(AR_SREV);
        let magic = hw.eeprom_read(0);
        if magic != Some(EEPROM_MAGIC) {
            kpi::dev_err(&format!("bad EEPROM magic {:?}", magic));
            return None;
        }
        let mut eeprom = alloc::vec![0u16; EEPROM_4K_WORDS];
        for (i, word) in eeprom.iter_mut().enumerate() {
            *word = hw.eeprom_read(EEPROM_4K_START + i as u32)?;
        }
        eeprom.swap(0, 2);
        let byte = |offset: usize| -> u8 {
            let word = eeprom[offset / 2];
            if offset % 2 == 0 { word as u8 } else { (word >> 8) as u8 }
        };
        let mut mac = [0u8; 6];
        for (i, b) in mac.iter_mut().enumerate() {
            *b = byte(12 + i);
        }
        let tx_gain_high = byte(31) == 1;
        let seed = kpi::io::uptime_ms().rotate_left(17) ^ ((hw.read(AR_TSF_L32) as u64) << 32) ^ u64::from_le_bytes([mac[0], mac[1], mac[2], mac[3], mac[4], mac[5], 0x5A, 0xA5]);
        let mut nic = Ar9285 {
            revision: (srev >> 8) & 0xF,
            eeprom_version: eeprom[0],
            hw,
            station: Station::new(seed),
            enabled: false,
            last_tick: 0,
            status: String::new(),
            id: 0,
            carrier: false,
        };
        nic.hw.mac = mac;
        match nic.reset(&eeprom, tx_gain_high) {
            Ok(()) => nic.status = String::from("radio ready"),
            Err(e) => {
                kpi::dev_err(&format!("reset failed: {}", e));
                nic.status = format!("hardware error: {}", e);
            }
        }
        kpi::dev_info(&format!(
            "{} srev {:#x} rev {} eeprom {:#06x} mac {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} tx gain {}",
            model,
            srev,
            nic.revision,
            nic.eeprom_version,
            mac[0],
            mac[1],
            mac[2],
            mac[3],
            mac[4],
            mac[5],
            if tx_gain_high { "high power" } else { "original" }
        ));
        Some(nic)
    }

    fn poll(&mut self) {
        if !self.hw.running || !self.enabled {
            return;
        }
        let now = kpi::io::uptime_ms();
        let mut frames = Vec::new();
        self.hw.collect_rx(&mut frames);
        for (frame, rssi) in frames {
            self.station.on_frame(&frame, rssi, now, &mut self.hw);
        }
        self.hw.service_tx();
        if now.saturating_sub(self.last_tick) >= 10 {
            self.last_tick = now;
            self.station.tick(now, &mut self.hw);
        }
        while let Some(frame) = self.station.take_ethernet() {
            kpi::net_receive(self.id, &frame);
        }
        let associated = self.station.associated();
        if associated != self.carrier {
            self.carrier = associated;
            kpi::net_carrier(self.id, associated);
        }
    }
}

fn security_code(security: wlan::Security) -> u8 {
    match security {
        wlan::Security::Open => kpi::WIFI_OPEN,
        wlan::Security::Wep => kpi::WIFI_WEP,
        wlan::Security::Wpa => kpi::WIFI_WPA,
        wlan::Security::Wpa2 => kpi::WIFI_WPA2,
    }
}

extern "C" fn transmit(_context: u64, frame: *const u8, len: usize) -> i32 {
    let Some(nic) = nic() else {
        return -19;
    };
    if frame.is_null() || !nic.hw.running {
        return -22;
    }
    let data = unsafe { core::slice::from_raw_parts(frame, len) };
    if nic.station.send_ethernet(data, &mut nic.hw) { 0 } else { -11 }
}

extern "C" fn set_enabled(_context: u64, enabled: i32) {
    let Some(nic) = nic() else {
        return;
    };
    let enabled = enabled != 0;
    if enabled == nic.enabled {
        return;
    }
    nic.enabled = enabled;
    if nic.hw.running {
        nic.station.enable(enabled, &mut nic.hw);
    }
    if !enabled && nic.carrier {
        nic.carrier = false;
        kpi::net_carrier(nic.id, false);
    }
}

extern "C" fn scan(_context: u64) {
    if let Some(nic) = nic() {
        let now = kpi::io::uptime_ms();
        nic.station.start_scan(now, &mut nic.hw);
    }
}

extern "C" fn networks(_context: u64, out: *mut WifiBss, max: u32) -> i32 {
    let Some(nic) = nic() else {
        return -19;
    };
    if out.is_null() {
        return -22;
    }
    let mut count = 0usize;
    for bss in nic.station.networks().into_iter().take(max as usize) {
        let mut entry = WifiBss { ssid: [0; 32], ssid_len: 0, bssid: bss.bssid, channel: bss.channel, security: security_code(bss.security), signal: bss.signal_percent(), _pad: 0, last_seen: bss.last_seen };
        let bytes = bss.ssid.as_bytes();
        let n = bytes.len().min(32);
        entry.ssid[..n].copy_from_slice(&bytes[..n]);
        entry.ssid_len = n as u32;
        unsafe { out.add(count).write(entry) };
        count += 1;
    }
    count as i32
}

extern "C" fn connect(_context: u64, ssid: *const u8, ssid_len: usize, pass: *const u8, pass_len: usize) -> i32 {
    let Some(nic) = nic() else {
        return -19;
    };
    if !nic.hw.running {
        return -5;
    }
    if ssid.is_null() || (pass.is_null() && pass_len > 0) {
        return -22;
    }
    let ssid = unsafe { core::slice::from_raw_parts(ssid, ssid_len.min(32)) };
    let pass = if pass_len == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(pass, pass_len.min(64)) } };
    let (Ok(ssid), Ok(pass)) = (core::str::from_utf8(ssid), core::str::from_utf8(pass)) else {
        return -22;
    };
    let now = kpi::io::uptime_ms();
    match nic.station.connect(ssid, pass, now, &mut nic.hw) {
        Ok(()) => 0,
        Err("Wi-Fi is off") => -19,
        Err("bad SSID") => -22,
        Err(e) if e.contains("passphrase") => -34,
        Err(_) => -1,
    }
}

extern "C" fn disconnect(_context: u64) {
    if let Some(nic) = nic() {
        nic.station.disconnect(&mut nic.hw);
    }
}

extern "C" fn status(_context: u64, out: *mut WifiStatus) -> i32 {
    let Some(nic) = nic() else {
        return -19;
    };
    if out.is_null() {
        return -22;
    }
    let status = unsafe { &mut *out };
    status.associated = nic.station.associated() as u32;
    status.signal = nic.station.signal();
    if let Some(ssid) = nic.station.ssid().filter(|_| nic.station.associated()) {
        status.set_ssid(ssid.as_bytes());
    }
    let text = if nic.hw.running { nic.station.status_text() } else { nic.status.clone() };
    status.set_text(text.as_bytes());
    0
}

extern "C" fn poll(_context: *mut c_void) {
    if let Some(nic) = nic() {
        nic.poll();
    }
}

fn init() -> i32 {
    for (device, model) in IDS {
        let mut handle = PciHandle::default();
        if unsafe { kpi::hamix_pci_find(0x168C, *device, &mut handle) } != 0 {
            continue;
        }
        let Some(started) = Ar9285::start(&handle, model) else {
            continue;
        };
        let mut mac = [0u8; 8];
        mac[..6].copy_from_slice(&started.hw.mac);
        unsafe { *(&raw mut NIC) = Some(started) };
        let ops = WifiNetOps {
            abi: kpi::NET_ABI_WIFI,
            flags: kpi::NET_FLAG_WIFI,
            context: 0,
            mac,
            transmit: Some(transmit),
            set_enabled: Some(set_enabled),
            wifi: WifiOps { scan: Some(scan), networks: Some(networks), connect: Some(connect), disconnect: Some(disconnect), status: Some(status) },
        };
        let id = kpi::register_wifi(&format!("ar9285 ({})", model), &ops);
        if id <= 0 {
            unsafe { *(&raw mut NIC) = None };
            return -1;
        }
        if let Some(running) = nic() {
            running.id = id;
        }
        if !kpi::claim(kpi::CLASS_NETWORK, "ar9285", &handle, Some(poll), core::ptr::null_mut()) {
            return -1;
        }
        return 0;
    }
    -19
}

fn exit() {
    if let Some(nic) = nic() {
        if nic.hw.running {
            nic.station.enable(false, &mut nic.hw);
        }
        nic.hw.write(AR_IMR, 0);
        nic.hw.write(AR_CR, 0x20);
        nic.hw.running = false;
        unsafe { kpi::hamix_unregister_netdev(nic.id) };
    }
    unsafe { *(&raw mut NIC) = None };
}

hamix_kpi::kernel_heap!();

hamix_kpi::module!(init = init, exit = exit, version = "1.0");
