pub struct Model {
    pub device: u16,
    pub name: &'static str,
    pub codename: &'static str,
    pub g4x: bool,
}

pub const MODELS: &[Model] = &[
    Model { device: 0x2A42, name: "Intel GMA 4500MHD", codename: "Mobile 4 Series (Cantiga)", g4x: true },
    Model { device: 0x2E02, name: "Intel GMA 4500", codename: "4 Series (Eaglelake)", g4x: true },
    Model { device: 0x2E12, name: "Intel GMA 4500", codename: "Q45/Q43 (Eaglelake)", g4x: true },
    Model { device: 0x2E22, name: "Intel GMA X4500HD", codename: "G45/G43 (Eaglelake)", g4x: true },
    Model { device: 0x2E32, name: "Intel GMA X4500", codename: "G41 (Eaglelake)", g4x: true },
    Model { device: 0x2E42, name: "Intel GMA 4500", codename: "B43 (Eaglelake)", g4x: true },
    Model { device: 0x2E92, name: "Intel GMA 4500", codename: "B43 (Eaglelake)", g4x: true },
    Model { device: 0x2A02, name: "Intel GMA X3100", codename: "GM965 (Crestline)", g4x: false },
    Model { device: 0x2A12, name: "Intel GMA X3100", codename: "GME965 (Crestline)", g4x: false },
    Model { device: 0x2972, name: "Intel GMA 3000", codename: "946GZ (Broadwater)", g4x: false },
    Model { device: 0x2982, name: "Intel GMA X3500", codename: "G35 (Broadwater)", g4x: false },
    Model { device: 0x2992, name: "Intel GMA 3000", codename: "Q965 (Broadwater)", g4x: false },
    Model { device: 0x29A2, name: "Intel GMA X3000", codename: "G965 (Broadwater)", g4x: false },
];
