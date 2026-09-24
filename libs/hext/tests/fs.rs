use std::process::Command;

use hext::{BlockDevice, Error, FormatOptions, Hext, VecDevice, ROOT_INO, S_IFDIR, S_IFREG};

struct Faulty {
    inner: VecDevice,
    budget: usize,
}

impl BlockDevice for Faulty {
    fn read(&mut self, sector: u64, buf: &mut [u8]) -> Result<(), Error> {
        self.inner.read(sector, buf)
    }

    fn write(&mut self, sector: u64, buf: &[u8]) -> Result<(), Error> {
        if self.budget == 0 {
            return Ok(());
        }
        self.budget -= 1;
        self.inner.write(sector, buf)
    }

    fn flush(&mut self) -> Result<(), Error> {
        Ok(())
    }

    fn sectors(&self) -> u64 {
        self.inner.sectors()
    }
}

struct Counting {
    inner: VecDevice,
    writes: usize,
}

impl BlockDevice for Counting {
    fn read(&mut self, sector: u64, buf: &mut [u8]) -> Result<(), Error> {
        self.inner.read(sector, buf)
    }

    fn write(&mut self, sector: u64, buf: &[u8]) -> Result<(), Error> {
        self.writes += 1;
        self.inner.write(sector, buf)
    }

    fn flush(&mut self) -> Result<(), Error> {
        Ok(())
    }

    fn sectors(&self) -> u64 {
        self.inner.sectors()
    }
}

fn options() -> FormatOptions {
    FormatOptions { label: "test".into(), seed: 7, now: 1_700_000_000 }
}

fn fsck(image: &[u8], name: &str) {
    if Command::new("e2fsck").arg("-V").output().is_err() {
        return;
    }
    let path = std::env::temp_dir().join(format!("hext-{}-{}.img", name, std::process::id()));
    std::fs::write(&path, image).unwrap();
    let out = Command::new("e2fsck").args(["-f", "-n"]).arg(&path).output().unwrap();
    let _ = std::fs::remove_file(&path);
    assert!(
        out.status.success(),
        "e2fsck reported problems for {}:\n{}{}",
        name,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn format_and_roundtrip() {
    let mut fs = Hext::format(VecDevice::new(16 << 20), options()).unwrap();
    let docs = fs.create(ROOT_INO, "docs", S_IFDIR | 0o755, 1000, 1000).unwrap();
    let file = fs.create(docs, "hello.txt", S_IFREG | 0o644, 1000, 1000).unwrap();
    fs.write_file(file, b"hello hext").unwrap();
    let big: Vec<u8> = (0..3_000_000u32).map(|i| (i * 7 % 251) as u8).collect();
    let big_ino = fs.create(ROOT_INO, "big.bin", S_IFREG | 0o600, 0, 0).unwrap();
    fs.write_file(big_ino, &big).unwrap();
    for i in 0..300 {
        let f = fs.create(docs, &format!("note-{:03}.txt", i), S_IFREG | 0o644, 0, 0).unwrap();
        fs.write_file(f, format!("note {}", i).as_bytes()).unwrap();
    }
    fs.commit().unwrap();
    let device = fs.into_device();
    fsck(&device.data, "roundtrip");

    let mut fs = Hext::mount(device, 1_700_000_100).unwrap();
    let docs = fs.resolve("/docs").unwrap();
    assert_eq!(fs.read_dir(docs).unwrap().len(), 301);
    let hello = fs.resolve("/docs/hello.txt").unwrap();
    assert_eq!(fs.read_file(hello).unwrap(), b"hello hext");
    let big_ino = fs.resolve("/big.bin").unwrap();
    assert_eq!(fs.read_file(big_ino).unwrap(), big);
    let note = fs.resolve("/docs/note-123.txt").unwrap();
    assert_eq!(fs.read_file(note).unwrap(), b"note 123");

    for i in 0..300 {
        fs.unlink(docs, &format!("note-{:03}.txt", i)).unwrap();
    }
    fs.write_file(big_ino, b"small now").unwrap();
    fs.commit().unwrap();
    let free_after = fs.stats().free_blocks;
    let device = fs.into_device();
    fsck(&device.data, "after-unlink");
    let mut fs = Hext::mount(device, 1_700_000_200).unwrap();
    assert_eq!(fs.stats().free_blocks, free_after);
    let big_ino = fs.resolve("/big.bin").unwrap();
    assert_eq!(fs.read_file(big_ino).unwrap(), b"small now");
    fs.remove_tree(ROOT_INO, "docs").unwrap();
    fs.commit().unwrap();
    fsck(&fs.into_device().data, "remove-tree");
}

#[test]
fn large_bs_format() {
    let mut fs = Hext::format(VecDevice::new(600 << 20), options()).unwrap();
    assert_eq!(fs.block_size(), 4096);
    let f = fs.create(ROOT_INO, "x", S_IFREG | 0o644, 0, 0).unwrap();
    fs.write_file(f, &vec![0xAB; 50_000_000]).unwrap();
    fs.commit().unwrap();
    fsck(&fs.into_device().data, "large");
}

#[test]
fn power_loss_is_atomic() {
    let mut fs = Hext::format(VecDevice::new(8 << 20), options()).unwrap();
    let a = fs.create(ROOT_INO, "a.txt", S_IFREG | 0o644, 0, 0).unwrap();
    fs.write_file(a, b"old content").unwrap();
    let dir = fs.create(ROOT_INO, "dir", S_IFDIR | 0o755, 0, 0).unwrap();
    for i in 0..40 {
        let f = fs.create(dir, &format!("keep{}", i), S_IFREG | 0o644, 0, 0).unwrap();
        fs.write_file(f, b"keep").unwrap();
    }
    fs.commit().unwrap();
    let base = fs.into_device().data;

    let new_content: Vec<u8> = (0..20_000u32).map(|i| (i % 200) as u8).collect();

    let mut counting = Hext::mount(Counting { inner: VecDevice { data: base.clone() }, writes: 0 }, 1).unwrap();
    let start_writes = counting.device().writes;
    {
        let a = counting.resolve("/a.txt").unwrap();
        counting.write_file(a, &new_content).unwrap();
        let dir = counting.resolve("/dir").unwrap();
        counting.create(dir, "fresh", S_IFREG | 0o644, 0, 0).unwrap();
        counting.unlink(dir, "keep3").unwrap();
        counting.commit().unwrap();
    }
    let total = counting.device().writes - start_writes;
    assert!(total > 5);

    let mut saw_old = false;
    let mut saw_new = false;
    for budget in 0..=total {
        let mut fs = Hext::mount(Faulty { inner: VecDevice { data: base.clone() }, budget: usize::MAX }, 1).unwrap();
        fs.device().budget = budget;
        let a = fs.resolve("/a.txt").unwrap();
        fs.write_file(a, &new_content).unwrap();
        let dir = fs.resolve("/dir").unwrap();
        fs.create(dir, "fresh", S_IFREG | 0o644, 0, 0).unwrap();
        fs.unlink(dir, "keep3").unwrap();
        let _ = fs.commit();
        let image = fs.into_device().inner.data;

        let mut recovered = Hext::mount(VecDevice { data: image }, 2).expect("mount after power loss");
        let a_ino = recovered.resolve("/a.txt").unwrap();
        let content = recovered.read_file(a_ino).unwrap();
        let dir = recovered.resolve("/dir").unwrap();
        let names: Vec<String> = recovered.read_dir(dir).unwrap().into_iter().map(|e| e.name).collect();
        if content == b"old content" {
            saw_old = true;
            assert!(!names.contains(&"fresh".to_string()), "budget {}: partial transaction visible", budget);
            assert!(names.contains(&"keep3".to_string()));
        } else {
            assert_eq!(content, new_content, "budget {}: torn file", budget);
            saw_new = true;
            assert!(names.contains(&"fresh".to_string()), "budget {}: partial transaction visible", budget);
            assert!(!names.contains(&"keep3".to_string()));
        }
        let image = recovered.into_device().data;
        if budget % 3 == 0 || budget == total {
            fsck(&image, &format!("power-{}", budget));
        }
    }
    assert!(saw_old && saw_new);
}
