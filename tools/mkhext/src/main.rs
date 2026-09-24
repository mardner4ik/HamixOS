use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::exit;
use std::time::{SystemTime, UNIX_EPOCH};

use hext::{FormatOptions, Hext, VecDevice, S_IFDIR, S_IFREG};

fn parse_size(text: &str) -> Option<u64> {
    let (number, unit) = text.split_at(text.trim_end_matches(|c: char| c.is_ascii_alphabetic()).len());
    let base: u64 = number.parse().ok()?;
    let factor = match unit.to_ascii_uppercase().as_str() {
        "" | "B" => 1,
        "K" | "KB" | "KIB" => 1 << 10,
        "M" | "MB" | "MIB" => 1 << 20,
        "G" | "GB" | "GIB" => 1 << 30,
        _ => return None,
    };
    Some(base * factor)
}

fn content_size(path: &Path) -> u64 {
    let mut total = 0;
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            total += 2048;
            if meta.is_dir() {
                total += content_size(&entry.path());
            } else if meta.is_file() {
                total += meta.len();
            }
        }
    }
    total
}

fn owner_of(path: &Path, owners: &[(std::path::PathBuf, u32)]) -> u32 {
    owners.iter().filter(|(p, _)| path.starts_with(p)).map(|(_, u)| *u).next().unwrap_or(0)
}

fn copy_tree(fs_image: &mut Hext<VecDevice>, dir: &Path, ino: u32, files: &mut usize, owners: &[(std::path::PathBuf, u32)]) {
    let mut entries: Vec<_> = match fs::read_dir(dir) {
        Ok(e) => e.flatten().collect(),
        Err(e) => {
            eprintln!("mkhext: cannot read {}: {}", dir.display(), e);
            exit(1);
        }
    };
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let meta = match fs::symlink_metadata(entry.path()) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let perm = (meta.permissions().mode() & 0o7777) as u16;
        let uid = owner_of(&entry.path(), owners);
        if meta.is_dir() {
            let child = match fs_image.create(ino, &name, S_IFDIR | perm, uid, uid) {
                Ok(c) => c,
                Err(hext::Error::Exists) => fs_image.lookup(ino, &name).unwrap().unwrap(),
                Err(e) => {
                    eprintln!("mkhext: {}: {:?}", entry.path().display(), e);
                    exit(1);
                }
            };
            copy_tree(fs_image, &entry.path(), child, files, owners);
        } else if meta.is_file() {
            let data = fs::read(entry.path()).unwrap_or_default();
            let child = fs_image.create(ino, &name, S_IFREG | perm, uid, uid).unwrap_or_else(|e| {
                eprintln!("mkhext: {}: {:?}", entry.path().display(), e);
                exit(1);
            });
            fs_image.write_file(child, &data).unwrap_or_else(|e| {
                eprintln!("mkhext: {}: {:?}", entry.path().display(), e);
                exit(1);
            });
            *files += 1;
            if *files % 64 == 0 {
                fs_image.commit().expect("commit");
            }
        }
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let mut size: Option<u64> = None;
    let mut label = String::from("hamix");
    let mut positional = Vec::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--size" | "-s" => size = args.next().and_then(|s| parse_size(&s)),
            "--label" | "-L" => label = args.next().unwrap_or(label),
            "--help" | "-h" => {
                println!("usage: mkhext [--size N[K|M|G]] [--label NAME] <source-dir> <image>");
                return;
            }
            _ => positional.push(arg),
        }
    }
    if positional.len() != 2 {
        eprintln!("usage: mkhext [--size N[K|M|G]] [--label NAME] <source-dir> <image>");
        exit(2);
    }
    let source = Path::new(&positional[0]);
    let image = Path::new(&positional[1]);
    let needed = content_size(source);
    let size = size.unwrap_or_else(|| ((needed * 9 / 8 + (24 << 20)).max(32 << 20) + 0xFFFFF) & !0xFFFFF);
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);

    let device = VecDevice::new(size as usize);
    let mut fs_image = Hext::format(device, FormatOptions { label, seed: now ^ size, now: now as u32 }).unwrap_or_else(|e| {
        eprintln!("mkhext: format failed: {:?}", e);
        exit(1);
    });
    let mut files = 0usize;
    let mut owners = Vec::new();
    if let Ok(passwd) = fs::read_to_string(source.join("etc/passwd")) {
        for line in passwd.lines() {
            let f: Vec<&str> = line.split(':').collect();
            if f.len() >= 7 {
                if let Ok(uid) = f[2].parse::<u32>() {
                    if uid != 0 && f[5].starts_with("/home/") {
                        owners.push((source.join(f[5].trim_start_matches('/')), uid));
                    }
                }
            }
        }
    }
    copy_tree(&mut fs_image, source, hext::ROOT_INO, &mut files, &owners);
    fs_image.commit().unwrap_or_else(|e| {
        eprintln!("mkhext: commit failed: {:?}", e);
        exit(1);
    });
    let stats = fs_image.stats();
    let device = fs_image.into_device();
    let mut out = fs::File::create(image).unwrap_or_else(|e| {
        eprintln!("mkhext: {}: {}", image.display(), e);
        exit(1);
    });
    out.write_all(&device.data).expect("write image");
    println!(
        "mkhext: {} files, {} KiB image, {} of {} blocks free ({} B blocks), journal {} blocks",
        files,
        size / 1024,
        stats.free_blocks,
        stats.blocks,
        stats.block_size,
        stats.journal_blocks
    );
}
