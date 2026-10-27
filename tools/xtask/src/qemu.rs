use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::image::{Arch, Profile};
use crate::util::*;

pub fn iso_path(profile: Profile) -> PathBuf {
    out_dir().join(if profile.linux() { "hamix_os.iso" } else { "hamix_core.iso" })
}

fn kvm() -> bool {
    Path::new("/dev/kvm").exists() && fs::OpenOptions::new().read(true).write(true).open("/dev/kvm").is_ok()
}

pub fn base_command(arch: Arch, memory: &str) -> Result<Command> {
    let mut cmd = match arch {
        Arch::X86_64 => {
            let mut c = command("qemu-system-x86_64");
            c.args(["-M", "q35", "-m", memory]);
            if kvm() {
                c.arg("-enable-kvm");
            }
            c
        }
        Arch::Aarch64 => {
            let mut c = command("qemu-system-aarch64");
            let port = arch.port_out();
            c.args(["-M", "virt", "-cpu", "cortex-a72", "-m", memory, "-kernel"]).arg(port.join("Image")).arg("-initrd").arg(port.join("hext.img"));
            c
        }
        Arch::Riscv64 => {
            let mut c = command("qemu-system-riscv64");
            let port = arch.port_out();
            c.args(["-M", "virt", "-m", memory, "-kernel"]).arg(port.join("kernel.elf")).arg("-initrd").arg(port.join("hext.img"));
            c
        }
    };
    cmd.stdin(Stdio::null());
    Ok(cmd)
}

pub fn run(arch: Arch, profile: Profile, memory: &str, extra: &[String]) -> Result {
    let mut cmd = base_command(arch, memory)?;
    if arch == Arch::X86_64 {
        let iso = iso_path(profile);
        if !iso.exists() {
            return Err(format!("{} does not exist; run `cargo xtask build` first", iso.display()));
        }
        cmd.arg("-cdrom").arg(&iso);
        let disk = out_dir().join("hamix_disk.img");
        if disk.exists() {
            cmd.arg("-drive").arg(format!("file={},format=raw,if=none,id=d0", disk.display())).args(["-device", "ide-hd,drive=d0,bus=ide.0"]);
        }
        cmd.args(["-boot", "d", "-serial", "stdio"]);
    } else {
        cmd.arg("-nographic");
    }
    cmd.args(extra).stdin(Stdio::inherit());
    run_cmd(&mut cmd)
}

fn run_cmd(cmd: &mut Command) -> Result {
    crate::util::run(cmd)
}

fn test_iso(profile: Profile, script: &str) -> Result<PathBuf> {
    let iso_dir = out_dir().join("isoroot");
    if !iso_dir.join("boot/kernel.bin").exists() {
        return Err(String::from("out/isoroot is missing; run `cargo xtask build` first"));
    }
    let test_dir = out_dir().join("test/isoroot");
    let _ = fs::remove_dir_all(&test_dir);
    mkdirs(&test_dir)?;
    crate::bootloader::link_tree(&iso_dir, &test_dir)?;
    let grub = test_dir.join("boot/grub/grub.cfg");
    let _ = fs::remove_file(&grub);
    let linux = if profile.linux() { "" } else { " linux=0" };
    let safe = script.contains("-nomodules");
    let linux = if safe { format!("{} nomodules", linux) } else { String::from(linux) };
    let initfs = if safe || script.contains("-noinitfs") { "" } else { "    module2 /boot/initfs.tar initfs\n" };
    let linux = if script.contains("-ventoy") || script.contains("-vtoy-") { format!("{} livemedia=usb", linux) } else { linux };
    let linux = match std::env::var("HX_TEST_ARGS") {
        Ok(extra) if !extra.trim().is_empty() => format!("{} {}", linux, extra.trim()),
        _ => linux,
    };
    if script.contains("-vtoy-") {
        fs::write(test_dir.join("boot/vtoy.bin"), crate::ventoy::placeholder()).map_err(|e| e.to_string())?;
    }
    let _ = fs::remove_file(test_dir.join("boot/hxboot.cfg"));
    write(
        &test_dir.join("boot/hxboot.cfg"),
        &format!(
            "timeout 0\n{}entry HamixOS test\nkernel /boot/kernel.bin root=live console=ttyS0 hxtest={}{}\n{}",
            if script.contains("-vtoy-") { "ventoy-param /boot/vtoy.bin\n" } else { "" },
            script,
            linux,
            if initfs.is_empty() { "" } else { "module /boot/initfs.tar initfs\n" }
        ),
    )?;
    write(
        &grub,
        &format!(
            "set timeout=0\nset default=0\ninsmod multiboot2\ninsmod all_video\nset gfxmode=1024x768x32,auto\nset gfxpayload=keep\nmenuentry \"HamixOS test\" {{\n    multiboot2 /boot/kernel.bin root=live console=ttyS0 hxtest={}{}\n{}    boot\n}}\n",
            script, linux, initfs
        ),
    )?;
    let iso = out_dir().join("test/hamix_test.iso");
    crate::bootloader::make_iso(&test_dir, &iso, "HAMIXOS", crate::bootloader::Loader::current()?)?;
    Ok(iso)
}

fn media_images() -> Result<(PathBuf, PathBuf)> {
    let dir = out_dir().join("test/media");
    let _ = fs::remove_dir_all(&dir);
    let tree = dir.join("tree");
    mkdirs(&tree.join("Music/Long folder name"))?;
    write(&tree.join("README.TXT"), "HamixOS media test\n")?;
    write(&tree.join("Music/Long folder name/Ünicode file name.txt"), "joliet works\n")?;
    let big: String = (0..20000).map(|i| format!("line {}\n", i)).collect();
    write(&tree.join("big file.txt"), &big)?;
    let iso = dir.join("media.iso");
    crate::isoimage::write(&tree, &iso, "HXMEDIA", None)?;
    let fat = dir.join("fat.img");
    fs::File::create(&fat).and_then(|f| f.set_len(64 << 20)).map_err(|e| e.to_string())?;
    crate::util::run(command("mkfs.vfat").args(["-F", "32", "-n", "HXSTICK"]).arg(&fat))?;
    let image = format!("{}", fat.display());
    crate::util::run(command("mmd").args(["-i", &image, "::/Photos", "::/Photos/Holiday 2026"]))?;
    crate::util::run(command("mcopy").args(["-i", &image]).arg(tree.join("README.TXT")).arg("::/readme.txt"))?;
    crate::util::run(command("mcopy").args(["-i", &image]).arg(tree.join("big file.txt")).arg("::/Photos/Holiday 2026/A rather long file name.txt"))?;
    Ok((iso, fat))
}

pub struct Outcome {
    pub exit: Option<i32>,
    pub log: String,
}

pub fn boot_script(profile: Profile, script: &str, memory: &str, timeout: Duration) -> Result<Outcome> {
    boot_script_smp(profile, script, memory, timeout, 1)
}

pub fn boot_script_smp(profile: Profile, script: &str, memory: &str, timeout: Duration, smp: u32) -> Result<Outcome> {
    let iso = test_iso(profile, script)?;
    let timeout = if kvm() { timeout } else { timeout * std::env::var("HX_TCG_FACTOR").ok().and_then(|f| f.parse().ok()).unwrap_or(4) };
    let log = out_dir().join("test/serial.log");
    let _ = fs::remove_file(&log);
    let mut cmd = base_command(Arch::X86_64, memory)?;
    if script.contains("-usbroot") || script.contains("-ventoy") {
        let stick = if script.contains("-ventoy") {
            let stick = out_dir().join("test/ventoy.img");
            crate::exfat::ventoy_stick(&stick, &iso, "System Image.iso")?;
            stick
        } else {
            iso.clone()
        };
        cmd.arg("-drive").arg(format!("file={},format=raw,if=none,id=stick,readonly=on", stick.display()));
        cmd.args([
            "-device", "ich9-usb-ehci1,id=ehci,addr=1d.7,multifunction=on",
            "-device", "ich9-usb-uhci1,masterbus=ehci.0,firstport=0,addr=1d.0,multifunction=on",
            "-device", "ich9-usb-uhci2,masterbus=ehci.0,firstport=2,addr=1d.1",
            "-device", "ich9-usb-uhci3,masterbus=ehci.0,firstport=4,addr=1d.2",
        ]);
        cmd.args(["-device", "usb-tablet,bus=ehci.0"]);
        let boot_from_stick = if script.contains("-ventoy") { "" } else { ",bootindex=0" };
        cmd.arg("-device").arg(format!("usb-storage,bus=ehci.0,drive=stick{}", boot_from_stick));
        if script.contains("-ventoy") {
            cmd.arg("-cdrom").arg(&iso).args(["-boot", "d"]);
        }
        cmd.args(["-display", "none", "-no-reboot", "-serial"]).arg(format!("file:{}", log.display()));
    } else if script.contains("-vtoy-") {
        let stick = out_dir().join("test/vtoy-ntfs.img");
        let param = out_dir().join("test/vtoy-param.bin");
        crate::ventoy::ntfs_stick(&stick, &iso, &param)?;
        crate::ventoy::patch_placeholder(&iso, &fs::read(&param).map_err(|e| e.to_string())?)?;
        cmd.arg("-drive").arg(format!("file={},format=raw,if=none,id=stick,readonly=on", stick.display()));
        cmd.args([
            "-device", "ich9-usb-ehci1,id=ehci,addr=1d.7,multifunction=on",
            "-device", "ich9-usb-uhci1,masterbus=ehci.0,firstport=0,addr=1d.0,multifunction=on",
            "-device", "usb-storage,bus=ehci.0,drive=stick",
        ]);
        cmd.arg("-cdrom").arg(&iso).args(["-boot", "d", "-display", "none", "-no-reboot", "-serial"]).arg(format!("file:{}", log.display()));
    } else {
        cmd.arg("-cdrom").arg(&iso).args(["-boot", "d", "-display", "none", "-no-reboot", "-serial"]).arg(format!("file:{}", log.display()));
    }
    if smp > 1 {
        cmd.arg("-smp").arg(smp.to_string());
    }
    if script.contains("-fakeos") {
        let disk = out_dir().join("test/fakeos.img");
        crate::ventoy::fake_windows_disk(&disk)?;
        cmd.arg("-drive").arg(format!("file={},format=raw,if=none,id=f0", disk.display())).args(["-device", "ide-hd,drive=f0,bus=ide.1"]);
    }
    if script.contains("-disk-") {
        let disk = out_dir().join("test/disk.img");
        let _ = fs::remove_file(&disk);
        fs::File::create(&disk).and_then(|f| f.set_len(2 << 30)).map_err(|e| format!("{}: {}", disk.display(), e))?;
        cmd.arg("-drive").arg(format!("file={},format=raw,if=none,id=d0", disk.display())).args(["-device", "ide-hd,drive=d0,bus=ide.0"]);
    }
    if script.contains("-media-") && !script.contains("-usb-") {
        let (iso, fat) = media_images()?;
        cmd.arg("-drive").arg(format!("file={},format=raw,if=none,id=m0,media=cdrom,readonly=on", iso.display())).args(["-device", "ide-cd,drive=m0,bus=ide.3"]);
        cmd.arg("-drive").arg(format!("file={},format=raw,if=none,id=m1", fat.display())).args(["-device", "ide-hd,drive=m1,bus=ide.4"]);
    }
    if script.contains("-usb-") {
        let (iso, fat) = media_images()?;
        if script.contains("-media-") {
            cmd.arg("-drive").arg(format!("file={},format=raw,if=none,id=m0,media=cdrom,readonly=on", iso.display())).args(["-device", "ide-cd,drive=m0,bus=ide.3"]);
        }
        cmd.args(["-device", "ich9-usb-uhci1,id=uhci"]);
        cmd.arg("-drive").arg(format!("file={},format=raw,if=none,id=stick", fat.display())).args(["-device", "usb-storage,bus=uhci.0,drive=stick"]);
    }
    if script.contains("-ehci-hid") {
        cmd.args(["-device", "ich9-usb-ehci1,id=ehcihid", "-device", "usb-mouse,bus=ehcihid.0,usb_version=2", "-device", "usb-kbd,bus=ehcihid.0,usb_version=2"]);
    }
    if script.contains("-audio-") {
        let wav = out_dir().join("test/audio.wav");
        let _ = fs::remove_file(&wav);
        cmd.arg("-audiodev").arg(format!("wav,id=a0,path={},out.frequency=48000", wav.display())).args(["-device", "intel-hda", "-device", "hda-output,audiodev=a0"]);
    }
    if let Ok(monitor) = std::env::var("HX_QEMU_MONITOR") {
        cmd.arg("-monitor").arg(format!("unix:{},server,nowait", monitor));
    }
    if let Ok(qmp) = std::env::var("HX_QEMU_QMP") {
        cmd.arg("-qmp").arg(format!("unix:{},server,nowait", qmp));
    }
    if let Ok(extra) = std::env::var("HX_QEMU_EXTRA") {
        cmd.args(extra.split_whitespace());
    }
    cmd.stdout(Stdio::null()).stderr(Stdio::null());
    let mut child = cmd.spawn().map_err(|e| format!("qemu: {}", e))?;
    let started = Instant::now();
    let mut exit = None;
    loop {
        std::thread::sleep(Duration::from_millis(250));
        let text = fs::read_to_string(&log).unwrap_or_default();
        if let Some(line) = text.lines().rev().find(|l| l.starts_with("hxtest: exit ")) {
            exit = line.trim_start_matches("hxtest: exit ").trim().parse().ok();
        }
        if exit.is_some() || child.try_wait().map(|s| s.is_some()).unwrap_or(true) || started.elapsed() > timeout {
            break;
        }
    }
    std::thread::sleep(Duration::from_millis(500));
    let _ = child.kill();
    let _ = child.wait();
    let text = fs::read_to_string(&log).unwrap_or_default();
    if exit.is_none() {
        if let Some(line) = text.lines().rev().find(|l| l.starts_with("hxtest: exit ")) {
            exit = line.trim_start_matches("hxtest: exit ").trim().parse().ok();
        }
    }
    Ok(Outcome { exit, log: text })
}

pub fn test(profile: Profile, filter: Option<&str>, timeout: Duration, net: bool, smp: u32) -> Result {
    let dir = root().join("tests/boot");
    let mut scripts: Vec<String> = fs::read_dir(&dir)
        .map_err(|e| format!("{}: {}", dir.display(), e))?
        .filter_map(|e| e.ok())
        .map(|e| file_name(&e.path()))
        .filter(|n| n.ends_with(".sh"))
        .filter(|n| filter.map(|f| n.contains(f)).unwrap_or(true))
        .collect();
    scripts.sort();
    let desktop = fs::read_to_string(out_dir().join("rootfs/etc/hamix/desktop")).map(|d| String::from(d.trim())).unwrap_or_else(|_| String::from("all"));
    let mut failed = Vec::new();
    let mut ran = 0;
    for name in &scripts {
        let body = fs::read_to_string(dir.join(name)).unwrap_or_default();
        if desktop == "nook" && (name.contains("enlightenment") || body.contains("enlightenment")) {
            continue;
        }
        if desktop == "enlightenment" && ["hxserver", "hxperm-exec", "org.hamix.", "hxproto-check", "hxtoolkit"].iter().any(|k| body.contains(k)) {
            continue;
        }
        if !profile.linux() && name.starts_with("linux-") {
            continue;
        }
        if profile.linux() && name.starts_with("core-") {
            continue;
        }
        if name.contains("-net-") && !net {
            continue;
        }
        if name.starts_with("bench-") {
            continue;
        }
        let missing: Vec<&str> = [
            (name.contains("-ventoy"), "mkfs.exfat"),
            (name.contains("-media-") || name.contains("-usb-"), "mkfs.vfat"),
            (name.contains("-media-") || name.contains("-usb-"), "mcopy"),
        ]
        .iter()
        .filter(|(needed, tool)| *needed && !have(tool))
        .map(|(_, tool)| *tool)
        .collect();
        if !missing.is_empty() {
            println!("  skip {} (this host has no {})", name, missing.join(", "));
            continue;
        }
        if name.contains("-vtoy-") && crate::bootloader::Loader::current()? != crate::bootloader::Loader::Hxboot {
            println!("  skip {} (needs --loader hxboot)", name);
            continue;
        }
        if (name.starts_with("stress-") || name.starts_with("interactive-")) && filter.is_none() {
            continue;
        }
        ran += 1;
        say(&format!("test {} ({})", name, if profile.linux() { "hamix-full" } else { "hamix-core" }));
        let cpus = if name.starts_with("stress-") { smp.max(4) } else { smp };
        let outcome = boot_script_smp(profile, &format!("/usr/lib/hamix/tests/{}", name), "3G", timeout, cpus)?;
        let saved = out_dir().join("test").join(format!("{}.log", name));
        let _ = fs::write(&saved, &outcome.log);
        let bad: Vec<&str> = outcome.log.lines().filter(|l| l.starts_with("FAIL") || l.contains(": command not found") || (l.contains("syscall: unimplemented") && !name.contains("fuzz")) || l.contains("panicked at") || l.starts_with("bkl: cpu")).collect();
        match outcome.exit {
            Some(0) if bad.is_empty() => println!("  ok"),
            other => {
                println!("  FAILED (exit {:?}), log: {}", other, saved.display());
                for line in bad.iter().take(20) {
                    println!("    {}", line);
                }
                if other.is_none() {
                    for line in outcome.log.lines().rev().take(15).collect::<Vec<_>>().into_iter().rev() {
                        println!("    | {}", line);
                    }
                }
                failed.push(name.clone());
            }
        }
    }
    if failed.is_empty() {
        say(&format!("{} boot test(s) passed", ran));
        Ok(())
    } else {
        Err(format!("failed boot tests: {}", failed.join(", ")))
    }
}
