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

    pub fn from_code(code: u8) -> Security {
        match code {
            1 => Security::Wep,
            2 => Security::Wpa,
            3 => Security::Wpa2,
            _ => Security::Open,
        }
    }
}
