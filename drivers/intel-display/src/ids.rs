pub struct Model {
    pub device: u16,
    pub name: &'static str,
    pub codename: &'static str,
}

macro_rules! models {
    ($($device:literal, $codename:literal, $name:literal;)*) => {
        pub const MODELS: &[Model] = &[
            $(Model { device: $device, name: $name, codename: $codename }),*
        ];
    };
}

models! {
    0x0102, "Sandy Bridge", "Intel HD Graphics 2000";
    0x0106, "Sandy Bridge", "Intel HD Graphics 2000";
    0x010A, "Sandy Bridge", "Intel HD Graphics P3000";
    0x0112, "Sandy Bridge", "Intel HD Graphics 3000";
    0x0116, "Sandy Bridge", "Intel HD Graphics 3000";
    0x0122, "Sandy Bridge", "Intel HD Graphics 3000";
    0x0126, "Sandy Bridge", "Intel HD Graphics 3000";
    0x0152, "Ivy Bridge", "Intel HD Graphics 2500";
    0x0156, "Ivy Bridge", "Intel HD Graphics 2500";
    0x015A, "Ivy Bridge", "Intel HD Graphics P2500";
    0x0162, "Ivy Bridge", "Intel HD Graphics 4000";
    0x0166, "Ivy Bridge", "Intel HD Graphics 4000";
    0x016A, "Ivy Bridge", "Intel HD Graphics P4000";
    0x0172, "Ivy Bridge", "Intel HD Graphics";
    0x0176, "Ivy Bridge", "Intel HD Graphics";
    0x0402, "Haswell", "Intel HD Graphics";
    0x0406, "Haswell", "Intel HD Graphics";
    0x040A, "Haswell", "Intel HD Graphics";
    0x040B, "Haswell", "Intel HD Graphics";
    0x040E, "Haswell", "Intel HD Graphics";
    0x0412, "Haswell", "Intel HD Graphics 4600";
    0x0416, "Haswell", "Intel HD Graphics 4600";
    0x041A, "Haswell", "Intel HD Graphics P4600/P4700";
    0x041B, "Haswell", "Intel HD Graphics";
    0x041E, "Haswell", "Intel HD Graphics 4400";
    0x0A06, "Haswell", "Intel HD Graphics";
    0x0A0E, "Haswell", "Intel HD Graphics";
    0x0A16, "Haswell", "Intel HD Graphics 4400";
    0x0A1E, "Haswell", "Intel HD Graphics 4200";
    0x0A26, "Haswell", "Intel HD Graphics 5000";
    0x0A2E, "Haswell", "Intel Iris Graphics 5100";
    0x0D22, "Haswell", "Intel Iris Pro Graphics 5200";
    0x0D26, "Haswell", "Intel Iris Pro Graphics 5200";
    0x1606, "Broadwell", "Intel HD Graphics";
    0x160E, "Broadwell", "Intel HD Graphics";
    0x1612, "Broadwell", "Intel HD Graphics 5600";
    0x1616, "Broadwell", "Intel HD Graphics 5500";
    0x161E, "Broadwell", "Intel HD Graphics 5300";
    0x1622, "Broadwell", "Intel Iris Pro Graphics 6200";
    0x1626, "Broadwell", "Intel HD Graphics 6000";
    0x162A, "Broadwell", "Intel Iris Pro Graphics P6300";
    0x162B, "Broadwell", "Intel Iris Graphics 6100";
    0x162D, "Broadwell", "Intel Iris Pro Graphics P6300";
    0x1902, "Skylake", "Intel HD Graphics 510";
    0x1906, "Skylake", "Intel HD Graphics 510";
    0x190B, "Skylake", "Intel HD Graphics 510";
    0x190E, "Skylake", "Intel HD Graphics";
    0x1912, "Skylake", "Intel HD Graphics 530";
    0x1913, "Skylake", "Intel HD Graphics 510";
    0x1915, "Skylake", "Intel HD Graphics";
    0x1916, "Skylake", "Intel HD Graphics 520";
    0x1917, "Skylake", "Intel HD Graphics";
    0x191B, "Skylake", "Intel HD Graphics 530";
    0x191D, "Skylake", "Intel HD Graphics P530";
    0x191E, "Skylake", "Intel HD Graphics 515";
    0x1921, "Skylake", "Intel HD Graphics 520";
    0x1926, "Skylake", "Intel Iris Graphics 540";
    0x1927, "Skylake", "Intel Iris Graphics 550";
    0x192B, "Skylake", "Intel Iris Graphics 555";
    0x192D, "Skylake", "Intel Iris Graphics P555";
    0x1932, "Skylake", "Intel Iris Pro Graphics 580";
    0x193B, "Skylake", "Intel Iris Pro Graphics 580";
    0x193D, "Skylake", "Intel Iris Pro Graphics P580";
    0x5902, "Kaby Lake", "Intel HD Graphics 610";
    0x5906, "Kaby Lake", "Intel HD Graphics 610";
    0x590B, "Kaby Lake", "Intel HD Graphics 610";
    0x5912, "Kaby Lake", "Intel HD Graphics 630";
    0x5916, "Kaby Lake", "Intel HD Graphics 620";
    0x5917, "Kaby Lake", "Intel UHD Graphics 620";
    0x591B, "Kaby Lake", "Intel HD Graphics 630";
    0x591D, "Kaby Lake", "Intel HD Graphics P630";
    0x591E, "Kaby Lake", "Intel HD Graphics 615";
    0x5921, "Kaby Lake", "Intel HD Graphics 620";
    0x5926, "Kaby Lake", "Intel Iris Plus Graphics 640";
    0x5927, "Kaby Lake", "Intel Iris Plus Graphics 650";
    0x3E90, "Coffee Lake", "Intel UHD Graphics 610";
    0x3E91, "Coffee Lake", "Intel UHD Graphics 630";
    0x3E92, "Coffee Lake", "Intel UHD Graphics 630";
    0x3E9B, "Coffee Lake", "Intel UHD Graphics 630";
    0x3EA0, "Whiskey Lake", "Intel UHD Graphics 620";
    0x3EA5, "Coffee Lake", "Intel Iris Plus Graphics 655";
    0x9B41, "Comet Lake", "Intel UHD Graphics";
    0x9BC4, "Comet Lake", "Intel UHD Graphics";
    0x9BCA, "Comet Lake", "Intel UHD Graphics";
}
