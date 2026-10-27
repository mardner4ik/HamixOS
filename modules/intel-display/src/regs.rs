use hamix_kpi as kpi;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Gen {
    Gen6,
    Gen7,
    Gen7_5,
    Gen8,
    Gen9,
}

impl Gen {
    pub fn of(device: u16) -> Option<Gen> {
        Some(match device {
            0x0100..=0x012F => Gen::Gen6,
            0x0150..=0x017F => Gen::Gen7,
            0x0400..=0x043F | 0x0A00..=0x0A3F | 0x0C00..=0x0C3F | 0x0D00..=0x0D3F => Gen::Gen7_5,
            0x1600..=0x163F => Gen::Gen8,
            0x1900..=0x193F | 0x5900..=0x593F | 0x3E00..=0x3EFF | 0x9B00..=0x9BFF | 0x8A00..=0x8AFF => Gen::Gen9,
            _ => return None,
        })
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Gen::Gen6 => "gen6 Sandy Bridge",
            Gen::Gen7 => "gen7 Ivy Bridge",
            Gen::Gen7_5 => "gen7.5 Haswell",
            Gen::Gen8 => "gen8 Broadwell",
            Gen::Gen9 => "gen9 Skylake",
        }
    }

    pub fn generation(&self) -> &'static str {
        match self {
            Gen::Gen6 => "gen6",
            Gen::Gen7 => "gen7",
            Gen::Gen7_5 => "gen7.5",
            Gen::Gen8 => "gen8",
            Gen::Gen9 => "gen9",
        }
    }

    pub fn pipe_scaler(&self) -> bool {
        matches!(self, Gen::Gen9)
    }

    pub fn scaler_pipe_select(&self) -> bool {
        matches!(self, Gen::Gen7)
    }

    pub fn has_edp_transcoder(&self) -> bool {
        matches!(self, Gen::Gen7_5 | Gen::Gen8 | Gen::Gen9)
    }

}

pub const TRANS_A: u32 = 0x60000;
pub const TRANS_B: u32 = 0x61000;
pub const TRANS_C: u32 = 0x62000;
pub const TRANS_EDP: u32 = 0x6F000;

pub const HTOTAL: u32 = 0x00;
pub const VTOTAL: u32 = 0x0C;
pub const VBLANK: u32 = 0x10;
pub const VSYNC: u32 = 0x14;
pub const PIPESRC: u32 = 0x1C;

pub const CONF_A: u32 = 0x70008;
pub const PIPE_STRIDE: u32 = 0x1000;
pub const FRMCOUNT: u32 = 0x70040;
pub const PLANE_CTL: u32 = 0x70180;
pub const PLANE_SURF: u32 = 0x1C;

pub const PIPE_A: u32 = 0x60000;
pub const TRANS_DDI_FUNC_CTL_EDP: u32 = 0x6F400;

pub const PLANE_LINOFF: u32 = 0x04;
pub const PLANE_STRIDE: u32 = 0x08;
pub const PLANE_POS: u32 = 0x0C;
pub const PLANE_SIZE: u32 = 0x10;
pub const PLANE_OFFSET: u32 = 0x24;

pub const PF_CTL_A: u32 = 0x68080;
pub const PF_WIN_POS_A: u32 = 0x68070;
pub const PF_WIN_SZ_A: u32 = 0x68074;
pub const PF_STRIDE: u32 = 0x800;
pub const PF_ENABLE: u32 = 1 << 31;
pub const PF_FILTER_MED: u32 = 1 << 23;
pub const PF_PIPE_SEL_IVB: u32 = 29;

pub const PS_CTRL_A: u32 = 0x68180;
pub const PS_WIN_POS_A: u32 = 0x68170;
pub const PS_WIN_SZ_A: u32 = 0x68174;
pub const PS_STRIDE: u32 = 0x800;
pub const PS_SECOND: u32 = 0x100;
pub const PS_ENABLE: u32 = 1 << 31;
pub const PS_FILTER_MED: u32 = 0 << 23;

pub const CUR_CTL: u32 = 0x70080;
pub const CUR_BASE: u32 = 0x70084;
pub const CUR_POS: u32 = 0x70088;
pub const CURSOR_MODE_64_ARGB: u32 = 0x27;
pub const CURSOR_PIPE_SELECT: u32 = 1 << 28;

pub const ENABLE: u32 = 1 << 31;

pub struct Mmio {
    base: *mut u8,
    pub len: u64,
}

impl Mmio {
    pub fn new(base: *mut u8, len: u64) -> Mmio {
        Mmio { base, len }
    }

    pub fn read(&self, reg: u32) -> u32 {
        if self.base.is_null() || reg as u64 + 4 > self.len {
            return 0;
        }
        unsafe { kpi::hamix_readl(self.base.add(reg as usize) as *const u32) }
    }

    pub fn write(&self, reg: u32, value: u32) {
        if self.base.is_null() || reg as u64 + 4 > self.len {
            return;
        }
        unsafe { kpi::hamix_writel(value, self.base.add(reg as usize) as *mut u32) }
    }

    pub fn alive(&self) -> bool {
        !self.base.is_null() && self.read(0x0) != 0xFFFF_FFFF
    }
}

pub fn pair(value: u32) -> (u32, u32) {
    ((value & 0x1FFF) + 1, ((value >> 16) & 0x1FFF) + 1)
}

pub fn encode(active: u32, total: u32) -> u32 {
    (active.saturating_sub(1) & 0x1FFF) | ((total.saturating_sub(1) & 0x1FFF) << 16)
}

pub fn transcoder(chip: Gen, index: usize) -> u32 {
    match index {
        0 => TRANS_A,
        1 => TRANS_B,
        2 => TRANS_C,
        _ if chip.has_edp_transcoder() => TRANS_EDP,
        _ => TRANS_A,
    }
}

pub fn conf(chip: Gen, index: usize) -> u32 {
    match index {
        0 => CONF_A,
        1 => CONF_A + PIPE_STRIDE,
        2 => CONF_A + 2 * PIPE_STRIDE,
        _ if chip.has_edp_transcoder() => 0x7F008,
        _ => CONF_A,
    }
}

pub fn plane(pipe: usize) -> u32 {
    PLANE_CTL + (pipe.min(2) as u32) * PIPE_STRIDE
}

pub fn cursor(pipe: usize) -> u32 {
    CUR_CTL + (pipe.min(2) as u32) * PIPE_STRIDE
}

pub fn pipe_src(pipe: usize) -> u32 {
    PIPE_A + (pipe.min(2) as u32) * PIPE_STRIDE + PIPESRC
}

pub fn frames(pipe: usize) -> u32 {
    FRMCOUNT + (pipe.min(2) as u32) * PIPE_STRIDE
}
