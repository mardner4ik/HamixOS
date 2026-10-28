use std::io::Write;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let data = std::fs::read(&args[1]).unwrap();
    let w: u32 = args.get(2).and_then(|v| v.parse().ok()).unwrap_or(0);
    let h: u32 = args.get(3).and_then(|v| v.parse().ok()).unwrap_or(0);
    let start = std::time::Instant::now();
    match imago::decode_sized(&data, w, h) {
        Ok(b) => {
            let elapsed = start.elapsed();
            eprintln!("{} {}x{} {:?}", args[1], b.width, b.height, elapsed);
            let mut out = Vec::with_capacity(b.pixels.len() * 4 + 32);
            write!(out, "P7\nWIDTH {}\nHEIGHT {}\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\nENDHDR\n", b.width, b.height).unwrap();
            for p in b.pixels {
                out.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8, (p >> 24) as u8]);
            }
            std::io::stdout().write_all(&out).unwrap();
        }
        Err(e) => {
            eprintln!("{}: error {:?}", args[1], e);
            std::process::exit(1);
        }
    }
}
