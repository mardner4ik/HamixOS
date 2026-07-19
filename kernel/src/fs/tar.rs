use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;

pub struct TarEntry<'a> {
    pub name: String,
    pub is_dir: bool,
    pub data: &'a [u8],
}

fn parse_str(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

fn parse_octal(bytes: &[u8]) -> usize {
    let s = core::str::from_utf8(bytes).unwrap_or("0");
    let s = s.trim_matches(|c: char| c == '\0' || c == ' ');
    if s.is_empty() {
        return 0;
    }
    usize::from_str_radix(s, 8).unwrap_or(0)
}

pub fn parse(archive: &[u8]) -> Vec<TarEntry<'_>> {
    let mut out = Vec::new();
    let mut off = 0usize;

    while off + 512 <= archive.len() {
        let header = &archive[off..off + 512];
        if header.iter().all(|&b| b == 0) {
            break;
        }

        let name = parse_str(&header[0..100]);
        if name.is_empty() {
            break;
        }
        let size = parse_octal(&header[124..136]);
        let typeflag = header[156];
        let prefix = parse_str(&header[345..500]);
        let magic = &header[257..263];
        let is_ustar = magic == b"ustar\0" || magic == b"ustar ";

        let full_name = if is_ustar && !prefix.is_empty() {
            format!("{}/{}", prefix, name)
        } else {
            name
        };

        let data_start = off + 512;
        let data_end = (data_start + size).min(archive.len());
        let data = &archive[data_start..data_end];
        let is_dir = typeflag == b'5' || full_name.ends_with('/');

        out.push(TarEntry {
            name: full_name,
            is_dir,
            data,
        });

        let block_len = (size + 511) & !511;
        off = data_start + block_len;
    }

    out
}
