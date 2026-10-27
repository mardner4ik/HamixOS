use std::fs;
use std::path::Path;

use crate::isoimage::{self, Boot};
use crate::util::*;

#[derive(Clone, Copy, PartialEq)]
pub enum Loader {
    Grub,
    Hxboot,
}

impl Loader {
    pub fn parse(name: &str) -> Result<Loader> {
        match name {
            "grub" => Ok(Loader::Grub),
            "hxboot" => Ok(Loader::Hxboot),
            other => Err(format!("unknown loader '{}' (grub or hxboot)", other)),
        }
    }

    pub fn current() -> Result<Loader> {
        match std::env::var("HAMIX_LOADER") {
            Ok(name) if !name.is_empty() => Loader::parse(&name),
            _ => Ok(if have("grub-mkrescue") && have("xorriso") { Loader::Grub } else { Loader::Hxboot }),
        }
    }
}

pub fn build() -> Result<(Vec<u8>, Vec<u8>)> {
    say("Building hxboot (BIOS boot loader)...");
    let dir = root().join("boot/hxboot");
    run(crate::toolchain::cargo()?
        .current_dir(&dir)
        .env("CARGO_TARGET_DIR", root().join("target"))
        .args(["build", "--release", "-Z", "build-std=core,compiler_builtins", "-Z", "build-std-features=compiler-builtins-mem", "--target"])
        .arg(dir.join("i686-hxboot.json")))?;
    let elf = root().join("target/i686-hxboot/release/hxboot-bios");
    let work = root().join("target/hxboot");
    mkdirs(&work)?;
    let stage = work.join("hxboot.bin");
    let mbr = work.join("mbr.bin");
    let oc = crate::image::objcopy()?;
    run(command(&oc[0]).args(&oc[1..]).args(["-O", "binary", "--remove-section=.mbr"]).arg(&elf).arg(&stage))?;
    run(command(&oc[0]).args(&oc[1..]).args(["-O", "binary", "--only-section=.mbr"]).arg(&elf).arg(&mbr))?;
    let stage = fs::read(&stage).map_err(|e| format!("{}: {}", stage.display(), e))?;
    let mbr = fs::read(&mbr).map_err(|e| format!("{}: {}", mbr.display(), e))?;
    if stage.len() < 2048 || stage.len() > 0x28000 {
        return Err(format!("hxboot.bin has an unexpected size ({} bytes)", stage.len()));
    }
    Ok((stage, mbr))
}

pub fn make_iso(dir: &Path, iso: &Path, volume: &str, loader: Loader) -> Result {
    match loader {
        Loader::Grub => {
            if !have("grub-mkrescue") {
                return Err(String::from("grub-mkrescue is not installed; use --loader hxboot"));
            }
            if grub_mkrescue(iso, dir, &["--modules=multiboot2 normal video gfxterm vbe video_bochs video_cirrus all_video"]).is_err() {
                grub_mkrescue(iso, dir, &[])?;
            }
            Ok(())
        }
        Loader::Hxboot => {
            if !dir.join("boot/hxboot.cfg").exists() {
                return Err(format!("{}/boot/hxboot.cfg is missing", dir.display()));
            }
            let (stage, mbr) = build()?;
            fs::write(dir.join("boot/hxboot.bin"), &stage).map_err(|e| e.to_string())?;
            build_efi_image(dir)?;
            isoimage::write(dir, iso, volume, Some(&Boot { file: "boot/hxboot.bin", mbr: Some(&mbr), efi_file: Some("boot/efi.img") }))
        }
    }
}

fn build_efi_image(dir: &Path) -> Result {
    for tool in ["grub-mkstandalone", "mkfs.vfat", "mmd", "mcopy"] {
        if !have(tool) {
            return Err(format!("{} is required to add x86_64 UEFI boot support", tool));
        }
    }
    let work = root().join("target/hxboot-efi");
    mkdirs(&work)?;
    let config = work.join("grub.cfg");
    write(&config, "search --no-floppy --set=root --file /boot/kernel.bin\nset timeout=3\nset default=0\ninsmod multiboot2\ninsmod all_video\nset gfxmode=auto\nset gfxpayload=keep\nmenuentry 'HamixOS live' {\n    multiboot2 /boot/kernel.bin root=live\n    module2 /boot/hext.img hext\n    module2 /boot/initfs.tar initfs\n    boot\n}\nmenuentry 'HamixOS live (safe graphics)' {\n    multiboot2 /boot/kernel.bin root=live nomodules\n    module2 /boot/hext.img hext\n    module2 /boot/initfs.tar initfs\n    boot\n}\n")?;
    let efi = work.join("BOOTX64.EFI");
    run(command("grub-mkstandalone")
        .args(["-O", "x86_64-efi", "--modules=multiboot2 normal search search_fs_file iso9660 all_video", "--install-modules=multiboot2 normal search search_fs_file iso9660 all_video"])
        .arg("-o").arg(&efi)
        .arg(format!("boot/grub/grub.cfg={}", config.display())))?;
    let image = dir.join("boot/efi.img");
    if let Some(parent) = image.parent() {
        mkdirs(parent)?;
    }
    let _ = fs::remove_file(&image);
    run(command("mkfs.vfat").args(["-F", "16", "-C"]).arg(&image).arg("16384"))?;
    run(command("mmd").arg("-i").arg(&image).args(["::/EFI", "::/EFI/BOOT"]))?;
    run(command("mcopy").arg("-i").arg(&image).arg(&efi).arg("::/EFI/BOOT/BOOTX64.EFI"))?;
    Ok(())
}

pub fn link_tree(from: &Path, to: &Path) -> Result {
    mkdirs(to)?;
    for entry in fs::read_dir(from).map_err(|e| format!("{}: {}", from.display(), e))? {
        let path = entry.map_err(|e| e.to_string())?.path();
        let dest = to.join(file_name(&path));
        if path.is_dir() {
            link_tree(&path, &dest)?;
        } else if fs::hard_link(&path, &dest).is_err() {
            fs::copy(&path, &dest).map_err(|e| format!("{}: {}", path.display(), e))?;
        }
    }
    Ok(())
}
