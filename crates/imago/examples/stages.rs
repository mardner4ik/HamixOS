fn main() {
    let data = std::fs::read(std::env::args().nth(1).unwrap()).unwrap();
    for (w, h) in [(0u32, 0u32), (960, 540), (480, 270), (240, 135)] {
        let t = std::time::Instant::now();
        let mut size = (0, 0);
        for _ in 0..5 {
            let b = imago::decode_sized(&data, w, h).unwrap();
            size = (b.width, b.height);
        }
        println!("{}x{} -> {:?} {:?}", w, h, size, t.elapsed() / 5);
    }
}
