#![no_std]

extern crate alloc;

use alloc::format;
use alloc::vec::Vec;
use core::ffi::c_void;

use hamix_kpi::io::{delay_us, DmaRegion, Mmio};
use hamix_kpi::{self as kpi, NetOps, PciHandle};

const B0_CTST: u32 = 0x0004;
const B0_POWER_CTRL: u32 = 0x0007;
const B0_IMSK: u32 = 0x000C;
const B0_HWE_IMSK: u32 = 0x0014;
const B2_MAC_1: u32 = 0x0100;
const B2_MAC_CFG: u32 = 0x011A;
const B2_CHIP_ID: u32 = 0x011B;
const B2_E_0: u32 = 0x011C;
const B2_Y2_CLK_GATE: u32 = 0x011D;
const B2_Y2_CLK_CTRL: u32 = 0x0120;
const B2_TI_CTRL: u32 = 0x0138;
const B2_TST_CTRL1: u32 = 0x0158;
const B2_GP_IO: u32 = 0x015C;
const B2_I2C_IRQ: u32 = 0x0168;
const B3_RI_WTO_R1: u32 = 0x0190;
const B3_RI_CTRL: u32 = 0x01A0;
const TXA_CTRL: u32 = 0x0210;
const B8_Q_REGS: u32 = 0x0400;
const Y2_B8_PREF_REGS: u32 = 0x0450;
const B16_RAM_REGS: u32 = 0x0800;
const RX_GMF_CTRL_T: u32 = 0x0C48;
const RX_GMF_FL_MSK: u32 = 0x0C4C;
const RX_GMF_FL_THR: u32 = 0x0C50;
const RX_GMF_UP_THR: u32 = 0x0C58;
const RX_GMF_LP_THR: u32 = 0x0C5A;
const TX_GMF_EA: u32 = 0x0D40;
const TX_GMF_CTRL_T: u32 = 0x0D48;
const B28_DPT_CTRL: u32 = 0x0E08;
const GMAC_TI_ST_CTRL: u32 = 0x0E18;
const B28_Y2_ASF_STAT_CMD: u32 = 0x0E68;
const STAT_CTRL: u32 = 0x0E80;
const STAT_LAST_IDX: u32 = 0x0E84;
const STAT_LIST_ADDR_LO: u32 = 0x0E88;
const STAT_LIST_ADDR_HI: u32 = 0x0E8C;
const STAT_TX_IDX_TH: u32 = 0x0E98;
const STAT_PUT_IDX: u32 = 0x0E9C;
const STAT_FIFO_WM: u32 = 0x0EAC;
const STAT_FIFO_ISR_WM: u32 = 0x0EAD;
const STAT_LEV_TIMER_CTRL: u32 = 0x0EB8;
const STAT_TX_TIMER_INI: u32 = 0x0EC0;
const STAT_TX_TIMER_CTRL: u32 = 0x0EC8;
const STAT_ISR_TIMER_INI: u32 = 0x0ED0;
const STAT_ISR_TIMER_CTRL: u32 = 0x0ED8;
const GMAC_CTRL: u32 = 0x0F00;
const GPHY_CTRL: u32 = 0x0F04;
const GMAC_IRQ_SRC: u32 = 0x0F08;
const GMAC_LINK_CTRL: u32 = 0x0F10;
const Y2_CFG_SPC: u32 = 0x1C00;
const BASE_GMAC_1: u32 = 0x2800;

const GM_GP_CTRL: u32 = 0x04;
const GM_TX_CTRL: u32 = 0x08;
const GM_RX_CTRL: u32 = 0x0C;
const GM_TX_FLOW_CTRL: u32 = 0x10;
const GM_TX_PARAM: u32 = 0x14;
const GM_SERIAL_MODE: u32 = 0x18;
const GM_SRC_ADDR_1L: u32 = 0x1C;
const GM_SRC_ADDR_2L: u32 = 0x28;
const GM_MC_ADDR_H1: u32 = 0x34;
const GM_TX_IRQ_MSK: u32 = 0x50;
const GM_RX_IRQ_MSK: u32 = 0x54;
const GM_TR_IRQ_MSK: u32 = 0x58;
const GM_SMI_CTRL: u32 = 0x80;
const GM_SMI_DATA: u32 = 0x84;

const Q_R1: u32 = 0x0000;
const Q_XS1: u32 = 0x0200;
const Q_XA1: u32 = 0x0280;
const Q_CSR: u32 = 0x34;
const Q_WM: u32 = 0x40;
const RB_START: u32 = 0x00;
const RB_END: u32 = 0x04;
const RB_WP: u32 = 0x08;
const RB_RP: u32 = 0x0C;
const RB_RX_UTPP: u32 = 0x10;
const RB_RX_LTPP: u32 = 0x14;
const RB_CTRL: u32 = 0x28;
const PREF_CTRL: u32 = 0x00;
const PREF_LAST_IDX: u32 = 0x04;
const PREF_ADDR_LOW: u32 = 0x08;
const PREF_ADDR_HI: u32 = 0x0C;
const PREF_PUT_IDX: u32 = 0x14;

const PCI_STATUS: u32 = 0x06;
const PCI_OUR_REG_1: u32 = 0x40;
const PCI_OUR_REG_3: u32 = 0x80;
const PCI_OUR_REG_4: u32 = 0x84;
const PCI_OUR_REG_5: u32 = 0x88;
const PCI_CFG_REG_1: u32 = 0x94;
const PEX_UNC_ERR_STAT: u32 = 0x104;

const CS_RST_SET: u16 = 1 << 0;
const CS_RST_CLR: u16 = 1 << 1;
const CS_MRST_CLR: u16 = 1 << 3;
const Y2_LED_STAT_ON: u16 = 1 << 9;
const Y2_ASF_DISABLE: u16 = 1 << 12;
const Y2_HW_WOL_ON: u16 = 1 << 15;

const CHIP_ID_YUKON_XL: u8 = 0xB3;
const CHIP_ID_YUKON_EC_U: u8 = 0xB4;
const CHIP_ID_YUKON_EX: u8 = 0xB5;
const CHIP_ID_YUKON_EC: u8 = 0xB6;
const CHIP_ID_YUKON_FE: u8 = 0xB7;
const CHIP_ID_YUKON_FE_P: u8 = 0xB8;
const CHIP_ID_YUKON_SUPR: u8 = 0xB9;
const CHIP_ID_YUKON_OPT: u8 = 0xBC;

const HW_OWNER: u32 = 0x8000_0000;
const EOP: u32 = 0x0080_0000;
const OP_PACKET: u32 = 0x4100_0000;
const OP_RXSTAT: u32 = 0x6000_0000;
const OP_TXINDEXLE: u32 = 0x6800_0000;
const STLE_OP_MASK: u32 = 0xFF00_0000;

const GMR_FS_RX_OK: u32 = 1 << 8;
const GMR_FS_ANY_ERR: u32 = (1 << 0) | (1 << 1) | (1 << 3) | (1 << 4) | (1 << 5) | (1 << 6) | (1 << 7) | (1 << 11) | (1 << 12);

const MII_BMCR: u16 = 0;
const MII_ANAR: u16 = 4;
const PHY_MARV_PHY_STAT: u16 = 0x11;

const RX_COUNT: usize = 128;
const TX_COUNT: usize = 128;
const STAT_COUNT: usize = 1024;
const BUFFER: usize = 1536;

const IDS: &[(u16, &str)] = &[
    (0x4354, "Marvell Yukon 88E8040"),
    (0x4355, "Marvell Yukon 88E8040T"),
    (0x4351, "Marvell Yukon 88E8036"),
    (0x4352, "Marvell Yukon 88E8038"),
    (0x4353, "Marvell Yukon 88E8039"),
    (0x4357, "Marvell Yukon 88E8042"),
    (0x435A, "Marvell Yukon 88E8048"),
    (0x4361, "Marvell Yukon 88E8050"),
    (0x4362, "Marvell Yukon 88E8053"),
    (0x4363, "Marvell Yukon 88E8055"),
    (0x4364, "Marvell Yukon 88E8056"),
    (0x436A, "Marvell Yukon 88E8058"),
    (0x436B, "Marvell Yukon 88E8071"),
    (0x4370, "Marvell Yukon 88E8075"),
    (0x4380, "Marvell Yukon 88E8057"),
    (0x4381, "Marvell Yukon 88E8059"),
];

struct Yukon {
    io: Mmio,
    chip: u8,
    revision: u8,
    mac: [u8; 6],
    status_ring: DmaRegion,
    rx_ring: DmaRegion,
    tx_ring: DmaRegion,
    rx_buffers: DmaRegion,
    tx_buffers: DmaRegion,
    stat_cons: usize,
    rx_cons: usize,
    rx_put: usize,
    tx_prod: usize,
    tx_cons: usize,
    link: bool,
    speed: u32,
    full_duplex: bool,
    last_link_check: u64,
    id: i32,
    enabled: bool,
}

static mut NICS: Vec<Yukon> = Vec::new();

fn nics() -> &'static mut Vec<Yukon> {
    unsafe { &mut *(&raw mut NICS) }
}

impl Yukon {
    fn cfg_read32(&self, reg: u32) -> u32 {
        self.io.read32(Y2_CFG_SPC + reg)
    }

    fn cfg_write32(&self, reg: u32, value: u32) {
        self.io.write32(Y2_CFG_SPC + reg, value)
    }

    fn gm_read(&self, reg: u32) -> u16 {
        self.io.read16(BASE_GMAC_1 + reg)
    }

    fn gm_write(&self, reg: u32, value: u16) {
        self.io.write16(BASE_GMAC_1 + reg, value)
    }

    fn phy_read(&self, reg: u16) -> Option<u16> {
        self.gm_write(GM_SMI_CTRL, (reg << 6) | (1 << 5));
        for _ in 0..2000 {
            delay_us(10);
            if self.gm_read(GM_SMI_CTRL) & (1 << 4) != 0 {
                return Some(self.gm_read(GM_SMI_DATA));
            }
        }
        None
    }

    fn phy_write(&self, reg: u16, value: u16) {
        self.gm_write(GM_SMI_DATA, value);
        self.gm_write(GM_SMI_CTRL, reg << 6);
        for _ in 0..2000 {
            delay_us(10);
            if self.gm_read(GM_SMI_CTRL) & (1 << 3) == 0 {
                return;
            }
        }
    }

    fn set_prefetch(&self, queue: u32, addr: u64, last: u16) {
        let base = Y2_B8_PREF_REGS + queue;
        self.io.write32(base + PREF_CTRL, 1 << 0);
        self.io.write32(base + PREF_CTRL, 1 << 1);
        self.io.write32(base + PREF_ADDR_LOW, addr as u32);
        self.io.write32(base + PREF_ADDR_HI, (addr >> 32) as u32);
        self.io.write16(base + PREF_LAST_IDX, last);
        self.io.write32(base + PREF_CTRL, 1 << 3);
        let _ = self.io.read32(base + PREF_CTRL);
    }

    fn power_up(&self) {
        self.io.write8(B0_POWER_CTRL, (1 << 7) | (1 << 5) | (1 << 2) | (1 << 1));
        self.io.write32(B2_Y2_CLK_CTRL, 1);
        self.io.write8(B2_Y2_CLK_GATE, 0);
        let mut our = self.cfg_read32(PCI_OUR_REG_1);
        our &= !((1 << 26) | (1 << 27));
        if self.chip == CHIP_ID_YUKON_EC_U || self.chip == CHIP_ID_YUKON_EX || self.chip >= CHIP_ID_YUKON_FE_P {
            let v = self.cfg_read32(PCI_OUR_REG_4) & ((1 << 15) | (1 << 14) | (1 << 13) | (1 << 12));
            self.cfg_write32(PCI_OUR_REG_4, v);
            let v = self.cfg_read32(PCI_OUR_REG_5) & ((1 << 28) | (1 << 27));
            self.cfg_write32(PCI_OUR_REG_5, v);
            self.cfg_write32(PCI_CFG_REG_1, 0);
            self.io.write16(B0_CTST, Y2_HW_WOL_ON);
            let gpio = self.io.read32(B2_GP_IO) | (1 << 13);
            self.io.write32(B2_GP_IO, gpio);
            let _ = self.io.read32(B2_GP_IO);
        }
        self.cfg_write32(PCI_OUR_REG_1, our);
        self.io.write16(GMAC_LINK_CTRL, 1 << 0);
        self.io.write16(GMAC_LINK_CTRL, 1 << 1);
    }

    fn reset(&self) {
        if (CHIP_ID_YUKON_XL..=CHIP_ID_YUKON_SUPR).contains(&self.chip) {
            if self.chip != CHIP_ID_YUKON_EX && self.chip != CHIP_ID_YUKON_SUPR {
                self.io.write8(B28_Y2_ASF_STAT_CMD, 1 << 3);
            }
            self.io.write16(B0_CTST, Y2_ASF_DISABLE);
            self.io.write16(B0_CTST, CS_RST_SET);
            self.io.write16(B0_CTST, CS_RST_CLR);
        }
        self.io.write8(B2_TST_CTRL1, 1 << 1);
        let status = self.io.read16(Y2_CFG_SPC + PCI_STATUS);
        self.io.write16(Y2_CFG_SPC + PCI_STATUS, status | 0xF900);
        self.io.write16(B0_CTST, CS_MRST_CLR);
        self.cfg_write32(PEX_UNC_ERR_STAT, 0xFFFF_FFFF);
        self.power_up();
        self.io.write8(GPHY_CTRL, 1 << 0);
        self.io.write8(GPHY_CTRL, 1 << 1);
        self.io.write32(GMAC_CTRL, 1 << 0);
        self.io.write32(GMAC_CTRL, 1 << 1);
        self.io.write32(GMAC_CTRL, 1 << 4);
        if self.chip == CHIP_ID_YUKON_OPT && self.revision == 0 {
            self.io.write32(0x1100 + 0x00, (0x0080 << 16) | 0x0080);
        }
        self.io.write8(B2_TST_CTRL1, 1 << 0);
        self.io.write16(B0_CTST, Y2_LED_STAT_ON);
        self.io.write32(B2_I2C_IRQ, 1);
        self.io.write8(B2_TI_CTRL, 1 << 1);
        self.io.write8(B2_TI_CTRL, 1 << 0);
        self.io.write8(B28_DPT_CTRL, 1 << 0);
        self.io.write8(GMAC_TI_ST_CTRL, 1 << 1);
        self.io.write8(GMAC_TI_ST_CTRL, 1 << 0);
        if matches!(self.chip, CHIP_ID_YUKON_XL | CHIP_ID_YUKON_EC | CHIP_ID_YUKON_FE) {
            self.io.write16(B3_RI_CTRL, 1 << 0);
            self.io.write16(B3_RI_CTRL, 1 << 1);
            for offset in 0..12 {
                self.io.write8(B3_RI_WTO_R1 + offset, 36);
            }
        }
        self.io.write32(B0_HWE_IMSK, 0);
        let _ = self.io.read32(B0_HWE_IMSK);
        self.io.write32(B0_IMSK, 0);
        let _ = self.io.read32(B0_IMSK);

        self.io.write32(STAT_CTRL, 1 << 0);
        self.io.write32(STAT_CTRL, 1 << 1);
        self.io.write32(STAT_LIST_ADDR_LO, self.status_ring.phys as u32);
        self.io.write32(STAT_LIST_ADDR_HI, (self.status_ring.phys >> 32) as u32);
        self.io.write16(STAT_LAST_IDX, (STAT_COUNT - 1) as u16);
        self.io.write16(STAT_TX_IDX_TH, 0x0A);
        self.io.write8(STAT_FIFO_WM, 0x10);
        self.io.write8(STAT_FIFO_ISR_WM, 0x10);
        self.io.write32(STAT_ISR_TIMER_INI, 0x0190);
        let clock = match self.chip {
            CHIP_ID_YUKON_FE => 100,
            CHIP_ID_YUKON_FE_P => 50,
            _ => 125,
        };
        self.io.write32(STAT_TX_TIMER_INI, clock * 1000);
        self.io.write32(STAT_CTRL, 1 << 3);
        self.io.write8(STAT_TX_TIMER_CTRL, 1 << 2);
        self.io.write8(STAT_LEV_TIMER_CTRL, 1 << 2);
        self.io.write8(STAT_ISR_TIMER_CTRL, 1 << 2);
    }

    fn setup_rambuffer(&self) -> bool {
        let ram_kb = self.io.read8(B2_E_0) as u32 * 4;
        if ram_kb == 0 {
            return false;
        }
        let rx_size = ((ram_kb * 1024 * 2) / 3) / 1024 * 1024;
        let tx_size = ram_kb * 1024 - rx_size;
        let (rx_start, rx_end) = (0u32, rx_size - 1);
        let (tx_start, tx_end) = (rx_size, rx_size + tx_size - 1);
        let rxq = B16_RAM_REGS + Q_R1;
        self.io.write8(rxq + RB_CTRL, 1 << 1);
        self.io.write32(rxq + RB_START, rx_start / 8);
        self.io.write32(rxq + RB_END, rx_end / 8);
        self.io.write32(rxq + RB_WP, rx_start / 8);
        self.io.write32(rxq + RB_RP, rx_start / 8);
        let utpp = (rx_end + 1 - rx_start - 8 * 1024) / 8;
        let mut ltpp = (rx_end + 1 - rx_start - 16 * 1024) / 8;
        if rx_size < 10 {
            ltpp += (16 * 1024 - 10 * 1024) / 8;
        }
        self.io.write32(rxq + RB_RX_UTPP, utpp);
        self.io.write32(rxq + RB_RX_LTPP, ltpp);
        self.io.write8(rxq + RB_CTRL, 1 << 3);
        let _ = self.io.read8(rxq + RB_CTRL);
        let txq = B16_RAM_REGS + Q_XA1;
        self.io.write8(txq + RB_CTRL, 1 << 1);
        self.io.write32(txq + RB_START, tx_start / 8);
        self.io.write32(txq + RB_END, tx_end / 8);
        self.io.write32(txq + RB_WP, tx_start / 8);
        self.io.write32(txq + RB_RP, tx_start / 8);
        self.io.write8(txq + RB_CTRL, 1 << 5);
        self.io.write8(txq + RB_CTRL, 1 << 3);
        let _ = self.io.read8(txq + RB_CTRL);
        true
    }

    fn init_port(&mut self) {
        self.io.write32(GMAC_CTRL, 1 << 0);
        self.io.write32(GMAC_CTRL, 1 << 1);
        self.io.write32(GMAC_CTRL, 1 << 4);
        self.gm_write(GM_GP_CTRL, 0);
        let _ = self.io.read8(GMAC_IRQ_SRC);
        self.gm_write(GM_RX_CTRL, 1 << 13);
        self.gm_write(GM_TX_CTRL, (0x04 << 10) & (7 << 10));
        self.gm_write(GM_TX_FLOW_CTRL, 0xFFFF);
        self.gm_write(GM_TX_PARAM, ((0x03 << 14) & (3 << 14)) | ((0x0B << 9) & (0x1F << 9)) | ((0x1C << 4) & (0x1F << 4)) | 0x04);
        self.gm_write(GM_SERIAL_MODE, ((0x04 << 11) & (0x1F << 11)) | (1 << 9) | 0x1E);
        for (base, offset) in [(GM_SRC_ADDR_1L, 0), (GM_SRC_ADDR_2L, 0)] {
            for i in 0..3u32 {
                let lo = self.mac[(offset + i * 2) as usize] as u16;
                let hi = self.mac[(offset + i * 2 + 1) as usize] as u16;
                self.gm_write(base + i * 4, lo | (hi << 8));
            }
        }
        self.gm_write(GM_TX_IRQ_MSK, 0);
        self.gm_write(GM_RX_IRQ_MSK, 0);
        self.gm_write(GM_TR_IRQ_MSK, 0);

        self.io.write32(RX_GMF_CTRL_T, 1 << 0);
        self.io.write32(RX_GMF_CTRL_T, 1 << 1);
        let mut reg = (1 << 3) | (1 << 7);
        if self.chip == CHIP_ID_YUKON_FE_P || self.chip == CHIP_ID_YUKON_EX {
            reg |= 1 << 19;
        }
        self.io.write32(RX_GMF_CTRL_T, reg);
        for i in 0..4 {
            self.gm_write(GM_MC_ADDR_H1 + i * 4, 0xFFFF);
        }
        let mode = self.gm_read(GM_RX_CTRL) | (1 << 15) | (1 << 14);
        self.gm_write(GM_RX_CTRL, mode);
        if self.chip == CHIP_ID_YUKON_XL {
            self.io.write32(RX_GMF_FL_MSK, 0);
        } else {
            self.io.write32(RX_GMF_FL_MSK, GMR_FS_ANY_ERR);
        }
        let threshold = if self.chip == CHIP_ID_YUKON_FE_P && self.revision == 0 { 0x178 } else { 0x0A + 1 };
        self.io.write16(RX_GMF_FL_THR, threshold);
        self.io.write32(TX_GMF_CTRL_T, 1 << 0);
        self.io.write32(TX_GMF_CTRL_T, 1 << 1);
        self.io.write32(TX_GMF_CTRL_T, 1 << 3);

        let rambuf = self.io.read8(B2_E_0) != 0;
        if !rambuf {
            self.io.write16(RX_GMF_LP_THR, 0x0060);
            self.io.write16(RX_GMF_UP_THR, 0x0080);
            if (self.chip == CHIP_ID_YUKON_EX && self.revision != 0) || self.chip >= CHIP_ID_YUKON_SUPR {
                self.io.write32(TX_GMF_CTRL_T, 1 << 30);
            } else {
                self.io.write32(TX_GMF_CTRL_T, 1 << 30);
            }
        }
        if self.chip == CHIP_ID_YUKON_FE_P && self.revision == 0 {
            let ea = self.io.read32(TX_GMF_EA) & !0x03;
            self.io.write32(TX_GMF_EA, ea);
        }
        self.io.write8(TXA_CTRL, (1 << 6) | (1 << 4) | (1 << 2));
        self.io.write8(TXA_CTRL, 1 << 1);
        self.setup_rambuffer();
        self.io.write8(B16_RAM_REGS + Q_XS1 + RB_CTRL, 1 << 0);

        let bmu_clear = (1 << 4) | (1 << 2) | (1 << 1);
        let bmu_init = (1 << 11) | (1 << 10) | (1 << 8) | (1 << 5) | (1 << 3);
        let txq = B8_Q_REGS + Q_XA1;
        self.io.write32(txq + Q_CSR, bmu_clear);
        self.io.write32(txq + Q_CSR, bmu_init);
        self.io.write32(txq + Q_CSR, 1 << 7);
        self.io.write16(txq + Q_WM, 0x600);
        let rxq = B8_Q_REGS + Q_R1;
        self.io.write32(rxq + Q_CSR, bmu_clear);
        self.io.write32(rxq + Q_CSR, bmu_init);
        self.io.write32(rxq + Q_CSR, 1 << 7);
        self.io.write16(rxq + Q_WM, 0x600);

        self.set_prefetch(Q_XA1, self.tx_ring.phys, (TX_COUNT - 1) as u16);
        self.io.write32(rxq + Q_CSR, (1 << 14) | (1 << 12));
        self.set_prefetch(Q_R1, self.rx_ring.phys, (RX_COUNT - 1) as u16);
        for i in 0..RX_COUNT {
            let le = self.rx_ring.ptr::<u32>(i * 8);
            unsafe {
                *le = (self.rx_buffers.phys + (i * BUFFER) as u64) as u32;
                *le.add(1) = BUFFER as u32 | OP_PACKET | HW_OWNER;
            }
        }
        self.io.write16(Y2_B8_PREF_REGS + Q_R1 + PREF_PUT_IDX, (RX_COUNT - 1) as u16);

        let bmcr = self.phy_read(MII_BMCR).unwrap_or(0);
        self.phy_write(MII_BMCR, bmcr | 0x8000);
        for _ in 0..50 {
            delay_us(2000);
            if self.phy_read(MII_BMCR).map(|v| v & 0x8000 == 0).unwrap_or(true) {
                break;
            }
        }
        self.phy_write(MII_ANAR, 0x01E1);
        self.phy_write(MII_BMCR, 0x1000 | 0x0200);
    }

    fn update_link(&mut self) {
        let Some(status) = self.phy_read(PHY_MARV_PHY_STAT) else {
            return;
        };
        let up = status & (1 << 10) != 0 && status & (1 << 11) != 0;
        if up == self.link {
            return;
        }
        self.link = up;
        if up {
            self.speed = match (status >> 14) & 3 {
                2 => 1000,
                1 => 100,
                _ => 10,
            };
            self.full_duplex = status & (1 << 13) != 0;
            let mut gmac: u16 = (1 << 2) | (1 << 1) | (1 << 0);
            if self.speed == 1000 {
                gmac |= (1 << 7) | (1 << 3);
            } else if self.speed == 100 {
                gmac |= 1 << 3;
            }
            gmac |= (1 << 4) | (1 << 13);
            if self.full_duplex {
                gmac |= 1 << 5;
            }
            gmac |= (1 << 11) | (1 << 12);
            self.gm_write(GM_GP_CTRL, gmac);
            let _ = self.gm_read(GM_GP_CTRL);
            self.io.write32(GMAC_CTRL, 1 << 2);
            kpi::dev_info(&format!("link up {} Mbps {} duplex", self.speed, if self.full_duplex { "full" } else { "half" }));
            kpi::net_carrier(self.id, true);
        } else {
            let gmac = self.gm_read(GM_GP_CTRL) & !((1 << 11) | (1 << 12));
            self.gm_write(GM_GP_CTRL, gmac);
            kpi::dev_info("link down");
            kpi::net_carrier(self.id, false);
        }
    }

    fn refill_rx(&mut self) {
        let slot = self.rx_put;
        let rx_le = self.rx_ring.ptr::<u32>(slot * 8);
        unsafe {
            *rx_le = (self.rx_buffers.phys + (slot * BUFFER) as u64) as u32;
            *rx_le.add(1) = BUFFER as u32 | OP_PACKET | HW_OWNER;
        }
        self.rx_put = (self.rx_put + 1) % RX_COUNT;
        self.io.write16(Y2_B8_PREF_REGS + Q_R1 + PREF_PUT_IDX, self.rx_put as u16);
    }

    fn handle_status(&mut self) {
        let put = self.io.read16(STAT_PUT_IDX) as usize;
        let mut guard = 0;
        while self.stat_cons != put && guard < STAT_COUNT {
            guard += 1;
            let le = self.status_ring.ptr::<u32>(self.stat_cons * 8);
            let (status, control) = unsafe { (*le, *le.add(1)) };
            self.stat_cons = (self.stat_cons + 1) % STAT_COUNT;
            if control & HW_OWNER == 0 {
                continue;
            }
            unsafe { *le.add(1) = control & !HW_OWNER };
            match control & STLE_OP_MASK {
                OP_RXSTAT => {
                    let len = (control & 0xFFFF) as usize;
                    let frame_len = (status >> 16) as usize;
                    let index = self.rx_cons;
                    let bogus_status_chip = self.chip == CHIP_ID_YUKON_FE_P && self.revision == 0 && frame_len != len;
                    let ok = len <= BUFFER && len >= 14 && (bogus_status_chip || (status & GMR_FS_ANY_ERR == 0 && status & GMR_FS_RX_OK != 0 && frame_len == len));
                    if ok && self.enabled {
                        kpi::net_receive(self.id, self.rx_buffers.slice(index * BUFFER, len));
                    }
                    self.rx_cons = (self.rx_cons + 1) % RX_COUNT;
                    self.refill_rx();
                }
                OP_TXINDEXLE => {
                    self.tx_cons = (status & 0xFFF) as usize % TX_COUNT;
                }
                _ => {}
            }
        }
    }

    fn start(handle: &PciHandle) -> Option<Yukon> {
        let (base, len) = kpi::bar(handle, 0);
        let io = Mmio::map(base, len)?;
        unsafe { kpi::hamix_pci_enable(handle) };
        io.write32(Y2_CFG_SPC + PCI_OUR_REG_3, 0);
        io.write16(B0_CTST, CS_RST_CLR);
        let chip = io.read8(B2_CHIP_ID);
        if !(CHIP_ID_YUKON_XL..=CHIP_ID_YUKON_OPT).contains(&chip) {
            kpi::dev_err(&format!("unknown chip id {:#x}", chip));
            return None;
        }
        let revision = (io.read8(B2_MAC_CFG) >> 4) & 0x0F;
        let mut nic = Yukon {
            io,
            chip,
            revision,
            mac: [0; 6],
            status_ring: DmaRegion::new(STAT_COUNT * 8)?,
            rx_ring: DmaRegion::new(RX_COUNT * 8)?,
            tx_ring: DmaRegion::new(TX_COUNT * 8)?,
            rx_buffers: DmaRegion::new(RX_COUNT * BUFFER)?,
            tx_buffers: DmaRegion::new(TX_COUNT * BUFFER)?,
            stat_cons: 0,
            rx_cons: 0,
            rx_put: RX_COUNT - 1,
            tx_prod: 0,
            tx_cons: 0,
            link: false,
            speed: 0,
            full_duplex: false,
            last_link_check: 0,
            id: 0,
            enabled: true,
        };
        nic.reset();
        for i in 0..6 {
            nic.mac[i] = nic.io.read8(B2_MAC_1 + i as u32);
        }
        nic.init_port();
        Some(nic)
    }

    fn transmit(&mut self, frame: &[u8]) -> i32 {
        if !self.link || !self.enabled || frame.len() > BUFFER {
            return -11;
        }
        let next = (self.tx_prod + 1) % TX_COUNT;
        if next == self.tx_cons {
            self.handle_status();
            if next == self.tx_cons {
                return -11;
            }
        }
        let index = self.tx_prod;
        let mut len = frame.len();
        self.tx_buffers.slice(index * BUFFER, len).copy_from_slice(frame);
        if len < 60 {
            self.tx_buffers.slice(index * BUFFER + len, 60 - len).fill(0);
            len = 60;
        }
        let le = self.tx_ring.ptr::<u32>(index * 8);
        unsafe {
            *le = (self.tx_buffers.phys + (index * BUFFER) as u64) as u32;
            *le.add(1) = len as u32 | OP_PACKET | EOP | HW_OWNER;
        }
        self.tx_prod = next;
        self.io.write16(Y2_B8_PREF_REGS + Q_XA1 + PREF_PUT_IDX, next as u16);
        0
    }

    fn poll(&mut self) {
        let now = kpi::io::uptime_ms();
        if now.saturating_sub(self.last_link_check) >= 500 {
            self.last_link_check = now;
            self.update_link();
        }
        self.handle_status();
    }

    fn stop(&self) {
        self.io.write32(B8_Q_REGS + Q_R1 + Q_CSR, 1 << 3);
        self.io.write32(B8_Q_REGS + Q_XA1 + Q_CSR, 1 << 3);
        self.gm_write(GM_GP_CTRL, 0);
        self.io.write32(GMAC_CTRL, 1 << 0);
    }
}

extern "C" fn transmit(context: u64, frame: *const u8, len: usize) -> i32 {
    let Some(nic) = nics().get_mut(context as usize) else {
        return -19;
    };
    if frame.is_null() {
        return -22;
    }
    nic.transmit(unsafe { core::slice::from_raw_parts(frame, len) })
}

extern "C" fn set_enabled(context: u64, enabled: i32) {
    if let Some(nic) = nics().get_mut(context as usize) {
        nic.enabled = enabled != 0;
    }
}

extern "C" fn poll(context: *mut c_void) {
    if let Some(nic) = nics().get_mut(context as usize) {
        nic.poll();
    }
}

fn init() -> i32 {
    for (device, model) in IDS {
        let mut handle = PciHandle::default();
        if unsafe { kpi::hamix_pci_find(0x11AB, *device, &mut handle) } != 0 {
            continue;
        }
        let Some(mut nic) = Yukon::start(&handle) else {
            kpi::dev_err(&format!("{} did not initialise", model));
            continue;
        };
        let index = nics().len();
        let mut mac = [0u8; 8];
        mac[..6].copy_from_slice(&nic.mac);
        let ops = NetOps { abi: kpi::NET_ABI, flags: 0, context: index as u64, mac, transmit: Some(transmit), set_enabled: Some(set_enabled) };
        let id = kpi::register_netdev(model, &ops);
        if id <= 0 {
            nic.stop();
            continue;
        }
        nic.id = id;
        kpi::dev_info(&format!(
            "{} chip {:#x} rev {} mac {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} ram buffer {} KB",
            model,
            nic.chip,
            nic.revision,
            nic.mac[0],
            nic.mac[1],
            nic.mac[2],
            nic.mac[3],
            nic.mac[4],
            nic.mac[5],
            nic.io.read8(B2_E_0) as u32 * 4
        ));
        nics().push(nic);
        if !kpi::claim(kpi::CLASS_NETWORK, "yukon", &handle, Some(poll), index as *mut c_void) {
            return -1;
        }
    }
    if nics().is_empty() { -19 } else { 0 }
}

fn exit() {
    for nic in nics().iter() {
        nic.stop();
        unsafe { kpi::hamix_unregister_netdev(nic.id) };
    }
    nics().clear();
}

hamix_kpi::kernel_heap!();

hamix_kpi::module!(init = init, exit = exit, version = "1.0");
