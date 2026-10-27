use std::fs;
use std::path::{Path, PathBuf};

use crate::util::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Arch {
    X86_64,
    Aarch64,
    Riscv64,
}

impl Arch {
    pub fn parse(name: &str) -> Result<Arch> {
        match name {
            "x86_64" | "x86-64" | "amd64" => Ok(Arch::X86_64),
            "aarch64" | "arm64" => Ok(Arch::Aarch64),
            "riscv64" => Ok(Arch::Riscv64),
            _ => Err(format!("unknown architecture '{}'; supported: x86_64, aarch64, riscv64", name)),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Arch::X86_64 => "x86_64",
            Arch::Aarch64 => "aarch64",
            Arch::Riscv64 => "riscv64",
        }
    }

    fn kernel_target(self) -> String {
        match self {
            Arch::X86_64 => root().join("kernel/x86_64-hamix_os.json").display().to_string(),
            Arch::Aarch64 => String::from("aarch64-unknown-none-softfloat"),
            Arch::Riscv64 => String::from("riscv64imac-unknown-none-elf"),
        }
    }

    fn kernel_triple(self) -> &'static str {
        match self {
            Arch::X86_64 => "x86_64-hamix_os",
            Arch::Aarch64 => "aarch64-unknown-none-softfloat",
            Arch::Riscv64 => "riscv64imac-unknown-none-elf",
        }
    }

    fn legacy_app_target(self) -> String {
        match self {
            Arch::X86_64 => self.kernel_target(),
            Arch::Aarch64 => String::from("aarch64-unknown-none"),
            Arch::Riscv64 => String::from("riscv64gc-unknown-none-elf"),
        }
    }

    fn legacy_app_triple(self) -> &'static str {
        match self {
            Arch::X86_64 => "x86_64-hamix_os",
            Arch::Aarch64 => "aarch64-unknown-none",
            Arch::Riscv64 => "riscv64gc-unknown-none-elf",
        }
    }

    fn hamix_base(self) -> Option<&'static str> {
        match self {
            Arch::X86_64 => Some(r#""llvm-target": "x86_64-unknown-none",
  "data-layout": "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128",
  "arch": "x86_64",
  "target-pointer-width": 64,
  "features": "-mmx","#),
            Arch::Aarch64 => Some(r#""llvm-target": "aarch64-unknown-linux-gnu",
  "data-layout": "e-m:e-p270:32:32-p271:32:32-p272:64:64-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128-Fn32",
  "arch": "aarch64",
  "target-pointer-width": 64,
  "max-atomic-width": 128,
  "features": "+v8a","#),
            Arch::Riscv64 => Some(r#""llvm-target": "riscv64-unknown-linux-gnu",
  "data-layout": "e-m:e-p:64:64-i64:64-i128:128-n32:64-S128",
  "arch": "riscv64",
  "target-pointer-width": 64,
  "max-atomic-width": 64,
  "code-model": "medium",
  "llvm-abiname": "lp64d",
  "features": "+m,+a,+f,+d,+c,+zicsr,+zifencei","#),
        }
    }

    pub fn app_target(self) -> (String, String) {
        let sysroot = crate::hxlibc::sysroot(self).join("usr/lib");
        let Some(base) = self.hamix_base().filter(|_| sysroot.join("libhxc.so").exists()) else {
            return (self.legacy_app_target(), String::from(self.legacy_app_triple()));
        };
        let triple = format!("{}-hamix", self.name());
        let lib = sysroot.display().to_string();
        let bundled = sysroot.join(format!("libclang_rt.builtins-{}.a", self.name()));
        let builtins = if bundled.exists() {
            bundled.display().to_string()
        } else if let Some(host) = crate::hxlibc::host_builtins(self) {
            host.display().to_string()
        } else {
            match crate::hxlibc::alpine_builtins(self) {
                Ok(dir) => {
                    let _ = fs::copy(dir.join(format!("libclang_rt.builtins-{}.a", self.name())), &bundled);
                    bundled.display().to_string()
                }
                Err(_) => String::new(),
            }
        };
        let mut late = vec![String::from("\"-lhxc\"")];
        if !builtins.trim().is_empty() {
            late.push(format!("\"{}\"", builtins.trim()));
        }
        let spec = format!(
            r#"{{
  {base}
  "target-endian": "little",
  "target-c-int-width": 32,
  "os": "hamix",
  "executables": true,
  "linker-flavor": "gnu-lld",
  "linker": "rust-lld",
  "panic-strategy": "abort",
  "relocation-model": "pic",
  "position-independent-executables": true,
  "dynamic-linking": true,
  "pre-link-args": {{"gnu-lld": ["--dynamic-linker=/lib/ld.hx", "-z", "max-page-size=4096", "--build-id=none", "--as-needed", "-L{lib}"]}},
  "pre-link-objects": {{"dynamic-pic-exe": ["{lib}/Scrt1.o", "{lib}/crti.o"]}},
  "post-link-objects": {{"dynamic-pic-exe": ["{lib}/crtn.o"]}},
  "late-link-args": {{"gnu-lld": [{late}]}}
}}
"#,
            late = late.join(", ")
        );
        let dir = root().join("target/specs");
        let path = dir.join(format!("{}.json", triple));
        if fs::read_to_string(&path).ok().as_deref() != Some(spec.as_str()) {
            let _ = fs::create_dir_all(&dir);
            let _ = fs::write(&path, spec);
        }
        (path.display().to_string(), triple)
    }

    pub fn rootfs(self) -> PathBuf {
        match self {
            Arch::X86_64 => out_dir().join("rootfs"),
            _ => out_dir().join(format!("rootfs-{}", self.name())),
        }
    }

    pub fn port_out(self) -> PathBuf {
        out_dir().join(self.name())
    }

    pub fn kernel_bin(self) -> PathBuf {
        root().join("target").join(self.kernel_triple()).join("release/kernel")
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Profile {
    Full,
    Core,
}

impl Profile {
    pub fn parse(name: &str) -> Result<Profile> {
        match name {
            "full" | "hamix-full" => Ok(Profile::Full),
            "core" | "hamix-core" => Ok(Profile::Core),
            _ => Err(format!("unknown profile '{}'; supported: full, core", name)),
        }
    }

    pub fn linux(self) -> bool {
        self == Profile::Full
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Desktop {
    All,
    Nook,
    Enlightenment,
}

impl Desktop {
    pub fn parse(name: &str) -> Result<Desktop> {
        match name {
            "all" | "both" => Ok(Desktop::All),
            "nook" => Ok(Desktop::Nook),
            "enlightenment" | "e" => Ok(Desktop::Enlightenment),
            _ => Err(format!("unknown desktop '{}'; supported: all, nook, enlightenment", name)),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Desktop::All => "all",
            Desktop::Nook => "nook",
            Desktop::Enlightenment => "enlightenment",
        }
    }

    pub fn nook(self) -> bool {
        self != Desktop::Enlightenment
    }

    pub fn enlightenment(self) -> bool {
        self != Desktop::Nook
    }
}

pub const ENLIGHTENMENT_PORTS: [&str; 2] = ["enlightenment", "efl"];

pub struct Options {
    pub arch: Arch,
    pub profile: Profile,
    pub desktop: Desktop,
    pub extra: Option<PathBuf>,
}

#[derive(Clone)]
pub struct App {
    pub name: String,
    pub dir: PathBuf,
    pub linux_layer: bool,
    pub bins: Vec<String>,
    pub aliases: Vec<(String, String)>,
    pub libexec: Vec<String>,
    pub setuid: Vec<String>,
    pub nook: bool,
}

fn hamix_metadata(dir: &Path, key: &str) -> Option<String> {
    let manifest = fs::read_to_string(dir.join("Cargo.toml")).unwrap_or_default();
    let mut in_hamix = false;
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            in_hamix = line == "[package.metadata.hamix]";
        } else if in_hamix {
            if let Some((k, v)) = line.split_once('=') {
                if k.trim() == key {
                    return Some(String::from(v.trim()));
                }
            }
        }
    }
    None
}

fn is_disabled(dir: &Path) -> bool {
    hamix_metadata(dir, "disabled").as_deref() == Some("true")
}

fn metadata_list(dir: &Path, key: &str) -> Vec<String> {
    hamix_metadata(dir, key)
        .unwrap_or_default()
        .trim_matches(|c| c == '[' || c == ']')
        .split(',')
        .map(|item| item.trim().trim_matches('"').to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

fn app_aliases(dir: &Path) -> Vec<(String, String)> {
    hamix_metadata(dir, "aliases")
        .unwrap_or_default()
        .trim_matches(|c| c == '[' || c == ']')
        .split(',')
        .filter_map(|item| {
            let (alias, target) = item.trim().trim_matches('"').split_once('=')?;
            Some((String::from(alias), String::from(target)))
        })
        .collect()
}

fn app_bins(dir: &Path) -> Vec<String> {
    let mut bins = Vec::new();
    if dir.join("src/main.rs").exists() {
        bins.push(file_name(dir));
    }
    let mut extra: Vec<String> = fs::read_dir(dir.join("src/bin"))
        .map(|e| e.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().map(|x| x == "rs").unwrap_or(false)).map(|p| p.file_stem().unwrap().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    extra.sort();
    bins.extend(extra);
    bins
}

fn app_at(dir: PathBuf, linux_layer: bool) -> Option<App> {
    if !dir.join("Cargo.toml").exists() || is_disabled(&dir) {
        return None;
    }
    let bins = app_bins(&dir);
    if bins.is_empty() {
        return None;
    }
    let aliases = app_aliases(&dir);
    let libexec = if hamix_metadata(&dir, "install").as_deref() == Some("\"libexec\"") { bins.clone() } else { metadata_list(&dir, "libexec") };
    let setuid = metadata_list(&dir, "setuid");
    let nook = hamix_metadata(&dir, "desktop").as_deref() == Some("\"nook\"");
    Some(App { name: file_name(&dir), dir, linux_layer, bins, aliases, libexec, setuid, nook })
}

pub struct CLib {
    pub name: String,
    pub soversion: String,
    pub version: String,
    pub dir: PathBuf,
}

fn package_version(dir: &Path) -> String {
    let manifest = fs::read_to_string(dir.join("Cargo.toml")).unwrap_or_default();
    manifest.lines().map(str::trim).find_map(|l| l.strip_prefix("version").map(|v| v.trim_start_matches([' ', '=']).trim_matches('"').to_string())).unwrap_or_default()
}

pub fn clibs() -> Vec<CLib> {
    let mut libs = Vec::new();
    for group in subdirs(&root().join("userland")) {
        for dir in subdirs(&group) {
            if !dir.join("Cargo.toml").exists() || is_disabled(&dir) {
                continue;
            }
            let Some(name) = hamix_metadata(&dir, "clib").map(|v| v.trim_matches('"').to_string()) else {
                continue;
            };
            let soversion = hamix_metadata(&dir, "soversion").map(|v| v.trim_matches('"').to_string()).unwrap_or_else(|| String::from("0"));
            libs.push(CLib { name, soversion, version: package_version(&dir), dir });
        }
    }
    libs
}

fn install_clib(lib: &CLib, release: &Path, rootfs: &Path, stage: &Path) -> Result {
    let built = release.join(format!("lib{}.so", lib.name));
    if !built.exists() {
        return Err(format!("{} did not produce {}", lib.dir.display(), built.display()));
    }
    let soname = format!("lib{}.so.{}", lib.name, lib.soversion);
    let dev = format!("lib{}.so", lib.name);
    for base in [rootfs, stage] {
        let libdir = base.join("usr/lib");
        mkdirs(&libdir)?;
        install(&built, &libdir.join(&soname), "755")?;
    }
    let link = stage.join("usr/lib").join(&dev);
    let _ = fs::remove_file(&link);
    std::os::unix::fs::symlink(&soname, &link).map_err(|e| format!("{}: {}", link.display(), e))?;
    if lib.dir.join("include").is_dir() {
        copy_tree(&lib.dir.join("include"), &stage.join("usr/include"))?;
    }
    let template = lib.dir.join(format!("{}.pc.in", lib.name));
    if template.exists() {
        let text = fs::read_to_string(&template).map_err(|e| format!("{}: {}", template.display(), e))?;
        let pkgconfig = stage.join("usr/lib/pkgconfig");
        mkdirs(&pkgconfig)?;
        write(&pkgconfig.join(format!("{}.pc", lib.name)), &text.replace("@VERSION@", &lib.version))?;
    }
    Ok(())
}

pub fn disabled_apps() -> Vec<String> {
    let mut names = Vec::new();
    for group in subdirs(&root().join("userland")) {
        for dir in subdirs(&group) {
            if dir.join("Cargo.toml").exists() && is_disabled(&dir) {
                names.push(file_name(&dir));
            }
        }
    }
    names
}

pub fn apps() -> Vec<App> {
    let mut apps = Vec::new();
    for group in subdirs(&root().join("userland")) {
        for dir in subdirs(&group) {
            apps.extend(app_at(dir, false));
        }
    }
    let mut layer_dirs = subdirs(&root().join("layers/linux"));
    layer_dirs.extend(subdirs(&root().join("layers/linux/compat")));
    for dir in layer_dirs {
        apps.extend(app_at(dir, true));
    }
    apps
}

fn nightly_build(dir: &Path, target: &str) -> Result {
    run(crate::toolchain::cargo()?
        .current_dir(dir)
        .args(["build", "--release", "-Z", "build-std=core,compiler_builtins,alloc", "-Z", "build-std-features=compiler-builtins-mem", "--target", target]))
}

pub fn kernel(arch: Arch) -> Result<PathBuf> {
    say(&format!("Building the {} kernel...", arch.name()));
    nightly_build(&root().join("kernel"), &arch.kernel_target())?;
    let bin = arch.kernel_bin();
    if !bin.exists() {
        return Err(format!("kernel binary not found at {}", bin.display()));
    }
    if arch == Arch::X86_64 && have("grub-file") && run(command("grub-file").arg("--is-x86-multiboot2").arg(&bin)).is_err() {
        warn("grub-file multiboot2 check failed -- verify manually");
    }
    Ok(bin)
}

fn base_rootfs(opts: &Options, kernel_bin: &Path) -> Result<PathBuf> {
    say("Building the root filesystem...");
    let rootfs = opts.arch.rootfs();
    mkdirs(&rootfs)?;
    for entry in fs::read_dir(&rootfs).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        let name = file_name(&path);
        if name == "home" || name == "root" {
            continue;
        }
        if path.is_dir() && !path.is_symlink() {
            fs::remove_dir_all(&path).map_err(|e| format!("{}: {}", path.display(), e))?;
        } else {
            fs::remove_file(&path).map_err(|e| format!("{}: {}", path.display(), e))?;
        }
    }
    for d in [
        "bin", "etc", "etc/hamix", "dev", "proc", "sys", "tmp", "var", "usr", "home", "root", "lib", "mnt", "media", "boot", "run",
        "usr/bin", "usr/lib", "usr/libexec/hamix", "usr/lib/hamix/boot", "usr/share", "usr/share/doc", "var/log", "var/tmp", "var/cache", "home/user",
        "var/cache/nook", "home/user/Videos", "root/Videos", "lib/modules", "var/lib/hamix/commands", "var/lib/hamix/commands/bin",
    ] {
        mkdirs(&rootfs.join(d))?;
    }
    chmod(&rootfs.join("tmp"), 0o1777)?;
    chmod(&rootfs.join("var/cache/nook"), 0o1777)?;
    chmod(&rootfs.join("var/lib/hamix/commands"), 0o1777)?;
    chmod(&rootfs.join("var/lib/hamix/commands/bin"), 0o1777)?;
    chmod(&rootfs.join("root"), 0o700)?;
    chmod(&rootfs.join("home/user"), 0o700)?;
    fs::copy(kernel_bin, rootfs.join("boot/kernel.bin")).map_err(|e| e.to_string())?;
    if opts.arch == Arch::X86_64 {
        fs::copy(root().join("data/hxboot/hxboot.cfg"), rootfs.join("boot/hxboot.cfg")).map_err(|e| format!("hxboot.cfg: {}", e))?;
        mkdirs(&rootfs.join("usr/share/doc/hamix"))?;
        fs::copy(root().join("docs/reference/hxboot.md"), rootfs.join("usr/share/doc/hamix/hxboot.md")).map_err(|e| format!("hxboot.md: {}", e))?;
    }
    if opts.arch == Arch::X86_64 && !have("grub-mkimage") {
        warn("grub-mkimage is not installed: the image will have no GRUB for installed systems (Hamix Install needs it)");
    } else if opts.arch == Arch::X86_64 {
        say("Generating GRUB images for installed systems...");
        run(command("grub-mkimage")
            .args(["-O", "i386-pc", "-o"])
            .arg(rootfs.join("usr/lib/hamix/boot/core.img"))
            .args(["-p", "(,msdos1)/boot/grub"])
            .args("biosdisk part_msdos ext2 normal multiboot2 configfile all_video gfxterm video_bochs video_cirrus vbe vga echo test boot".split(' ')))?;
        fs::copy("/usr/lib/grub/i386-pc/boot.img", rootfs.join("usr/lib/hamix/boot/boot.img")).map_err(|e| format!("boot.img: {}", e))?;
    }
    Ok(rootfs)
}

pub const LIBEXEC: &str = "usr/libexec/hamix";

fn userland(opts: &Options, rootfs: &Path) -> Result<Vec<String>> {
    if opts.arch == Arch::X86_64 && !crate::hxlibc::sysroot(opts.arch).join("usr/lib/libhxc.so").exists() {
        crate::hxlibc::build(opts.arch)?;
    }
    let (target, triple) = opts.arch.app_target();
    say(&format!("Building userspace apps for {}...", triple));
    let mut built = Vec::new();
    for app in apps() {
        if app.linux_layer && !opts.profile.linux() {
            continue;
        }
        if app.nook && !opts.desktop.nook() {
            continue;
        }
        if !app.dir.join("build.rs").exists() {
            return Err(format!("{} has no build.rs: without userland/link.ld it links without the Hamix ELF note and runs as a Linux program", app.dir.display()));
        }
        if let Err(e) = nightly_build(&app.dir, &target) {
            if opts.arch == Arch::X86_64 {
                return Err(format!("{} does not build: {}", app.name, e));
            }
            warn(&format!("{} does not build for {}, skipping ({})", app.name, opts.arch.name(), e));
            continue;
        }
        for name in &app.bins {
            let bin = root().join("target").join(&triple).join("release").join(name);
            if !bin.exists() {
                warn(&format!("{} did not produce {}", app.name, bin.display()));
                continue;
            }
            let dir = if app.libexec.contains(name) { rootfs.join(LIBEXEC) } else { rootfs.join("usr/bin") };
            mkdirs(&dir)?;
            let mode = if app.setuid.contains(name) { "4755" } else { "755" };
            install(&bin, &dir.join(name), mode)?;
            built.push(name.clone());
        }
        if app.dir.join("share").is_dir() {
            copy_tree(&app.dir.join("share"), &rootfs.join("usr/share"))?;
        }
        for (alias, target) in &app.aliases {
            let installed = if app.libexec.contains(target) { rootfs.join(LIBEXEC).join(target) } else { rootfs.join("usr/bin").join(target) };
            if installed.exists() {
                let dest = match alias.strip_prefix('/') {
                    Some(absolute) => rootfs.join(absolute),
                    None => rootfs.join("usr/bin").join(alias),
                };
                install(&installed, &dest, "755")?;
            }
        }
    }
    let stage = crate::ports::stage_root(opts.arch);
    let release = root().join("target").join(&triple).join("release");
    for lib in clibs() {
        if let Err(e) = nightly_build(&lib.dir, &target).and_then(|_| install_clib(&lib, &release, rootfs, &stage)) {
            if opts.arch == Arch::X86_64 {
                return Err(format!("lib{} does not build: {}", lib.name, e));
            }
            warn(&format!("lib{} does not build for {}, skipping ({})", lib.name, opts.arch.name(), e));
            continue;
        }
        built.push(format!("lib{}.so.{}", lib.name, lib.soversion));
    }
    let disabled = disabled_apps();
    if !disabled.is_empty() {
        say(&format!("Disabled (kept in the tree, not installed): {}", disabled.join(", ")));
    }
    Ok(built)
}

fn kernel_exports() -> Result<Vec<String>> {
    let text = fs::read_to_string(root().join("kernel/src/module/symbols.rs")).map_err(|e| e.to_string())?;
    let mut names: Vec<String> = text
        .lines()
        .filter_map(|l| {
            let l = l.trim_start();
            let rest = l.strip_prefix('"')?;
            let end = rest.find('"')?;
            let name = &rest[..end];
            let tail = rest[end + 1..].trim_start();
            (tail.starts_with("=>") && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')).then(|| String::from(name))
        })
        .collect();
    names.sort();
    names.dedup();
    Ok(names)
}

fn check_symbols(name: &str, object: &Path, exports: &[String]) -> Result {
    let text = output(command("nm").arg("-u").arg(object))?;
    let mut missing: Vec<&str> = text
        .lines()
        .filter_map(|l| l.split_whitespace().last())
        .filter(|s| exports.binary_search(&String::from(*s)).is_err())
        .filter(|s| !(s.contains("panic") || s.ends_with("_fail") || *s == "rust_begin_unwind" || *s == "__stack_chk_fail"))
        .collect();
    missing.sort();
    missing.dedup();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!("module {} needs symbols the kernel does not export:\n    {}", name, missing.join("\n    ")))
    }
}

fn check_relocations(arch: Arch, name: &str, object: &Path) -> Result {
    if arch != Arch::X86_64 {
        return Ok(());
    }
    let text = output(command("readelf").arg("-r").arg(object))?;
    let absolute = text.lines().filter(|l| matches!(l.split_whitespace().nth(2), Some("R_X86_64_32") | Some("R_X86_64_32S"))).count();
    if absolute == 0 {
        Ok(())
    } else {
        Err(format!("module {} has {} 32-bit absolute relocations; it would fail to load above 2 GiB", name, absolute))
    }
}

fn sign(object: &Path, dest: &Path) -> Result {
    write(dest, &format!("{}\n", crate::util::sha256(object)?))
}

fn install_module(module_dir: &Path, name: &str, object: &Path, modules_dir: &Path) -> Result {
    install(object, &modules_dir.join(format!("{}.ko", name)), "644")?;
    sign(object, &modules_dir.join(format!("{}.sig", name)))?;
    for ext in ["conf"] {
        let src = module_dir.join(format!("module.{}", ext));
        if src.exists() {
            install(&src, &modules_dir.join(format!("{}.{}", name, ext)), "644")?;
        }
    }
    Ok(())
}

const FREESTANDING: [(&str, &str); 4] = [
    (
        "stddef.h",
        "#ifndef _HX_STDDEF_H\n#define _HX_STDDEF_H\ntypedef __SIZE_TYPE__ size_t;\ntypedef __PTRDIFF_TYPE__ ptrdiff_t;\ntypedef __WCHAR_TYPE__ wchar_t;\n#define NULL ((void *)0)\n#define offsetof(t, m) __builtin_offsetof(t, m)\n#endif\n",
    ),
    (
        "stdint.h",
        "#ifndef _HX_STDINT_H\n#define _HX_STDINT_H\ntypedef __INT8_TYPE__ int8_t;\ntypedef __INT16_TYPE__ int16_t;\ntypedef __INT32_TYPE__ int32_t;\ntypedef __INT64_TYPE__ int64_t;\ntypedef __UINT8_TYPE__ uint8_t;\ntypedef __UINT16_TYPE__ uint16_t;\ntypedef __UINT32_TYPE__ uint32_t;\ntypedef __UINT64_TYPE__ uint64_t;\ntypedef __INTPTR_TYPE__ intptr_t;\ntypedef __UINTPTR_TYPE__ uintptr_t;\ntypedef __INTMAX_TYPE__ intmax_t;\ntypedef __UINTMAX_TYPE__ uintmax_t;\n#define INT8_MAX __INT8_MAX__\n#define INT16_MAX __INT16_MAX__\n#define INT32_MAX __INT32_MAX__\n#define INT64_MAX __INT64_MAX__\n#define INT8_MIN (-INT8_MAX - 1)\n#define INT16_MIN (-INT16_MAX - 1)\n#define INT32_MIN (-INT32_MAX - 1)\n#define INT64_MIN (-INT64_MAX - 1)\n#define UINT8_MAX __UINT8_MAX__\n#define UINT16_MAX __UINT16_MAX__\n#define UINT32_MAX __UINT32_MAX__\n#define UINT64_MAX __UINT64_MAX__\n#define INTPTR_MAX __INTPTR_MAX__\n#define UINTPTR_MAX __UINTPTR_MAX__\n#define SIZE_MAX __SIZE_MAX__\n#define INT8_C(c) __INT8_C(c)\n#define INT16_C(c) __INT16_C(c)\n#define INT32_C(c) __INT32_C(c)\n#define INT64_C(c) __INT64_C(c)\n#define UINT8_C(c) __UINT8_C(c)\n#define UINT16_C(c) __UINT16_C(c)\n#define UINT32_C(c) __UINT32_C(c)\n#define UINT64_C(c) __UINT64_C(c)\n#endif\n",
    ),
    ("stdbool.h", "#ifndef _HX_STDBOOL_H\n#define _HX_STDBOOL_H\n#define bool _Bool\n#define true 1\n#define false 0\n#endif\n"),
    (
        "stdarg.h",
        "#ifndef _HX_STDARG_H\n#define _HX_STDARG_H\ntypedef __builtin_va_list va_list;\n#define va_start(ap, last) __builtin_va_start(ap, last)\n#define va_arg(ap, type) __builtin_va_arg(ap, type)\n#define va_end(ap) __builtin_va_end(ap)\n#define va_copy(d, s) __builtin_va_copy(d, s)\n#endif\n",
    ),
];

fn freestanding_headers(resource: &Path) -> Result<Option<PathBuf>> {
    if FREESTANDING.iter().all(|(name, _)| resource.join(name).exists()) {
        return Ok(None);
    }
    let dir = root().join("target/cmodules/include");
    for (name, text) in FREESTANDING {
        if !resource.join(name).exists() {
            write(&dir.join(name), text)?;
        }
    }
    Ok(Some(dir))
}

fn c_module(arch: Arch, name: &str, dir: &Path, modules_dir: &Path, exports: &[String]) -> Result {
    if !have("clang") {
        return Err(String::from("clang not found"));
    }
    let work = root().join("target/cmodules").join(arch.name()).join(name);
    let _ = fs::remove_dir_all(&work);
    mkdirs(&work)?;
    let flags: &[&str] = match arch {
        Arch::X86_64 => &["-target", "x86_64-unknown-none", "-mno-red-zone", "-mgeneral-regs-only", "-fpie"],
        Arch::Aarch64 => &["-target", "aarch64-unknown-none", "-mgeneral-regs-only", "-fno-pic"],
        Arch::Riscv64 => &["-target", "riscv64-unknown-elf", "-march=rv64imac", "-mabi=lp64", "-mcmodel=medany", "-fno-pic"],
    };
    let resource = format!("{}/include", output(command("clang").arg("-print-resource-dir"))?.trim());
    let freestanding = freestanding_headers(Path::new(&resource))?;
    let kpi = root().join("sdk/hamix_linuxkpi");
    let mut sources: Vec<PathBuf> = fs::read_dir(dir.join("src")).map_err(|e| e.to_string())?.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().map(|x| x == "c").unwrap_or(false)).collect();
    sources.sort();
    sources.push(kpi.join("src/linuxkpi.c"));
    let mut objects = Vec::new();
    for source in sources {
        let object = work.join(format!("{}.o", source.file_stem().unwrap().to_string_lossy()));
        run(command("clang")
            .args(flags)
            .args(["-ffreestanding", "-fno-builtin", "-nostdinc"])
            .args(freestanding.iter().flat_map(|d| [String::from("-isystem"), d.display().to_string()]))
            .args(["-isystem", &resource])
            .arg(format!("-I{}", kpi.join("include").display()))
            .arg(format!("-I{}", dir.join("src").display()))
            .args(["-O2", "-fno-stack-protector", "-fno-common", "-fno-asynchronous-unwind-tables", "-std=gnu11", "-Wno-unused-function", "-Wno-pointer-sign"])
            .arg(format!("-DKBUILD_MODNAME=\"{}\"", name))
            .arg("-c")
            .arg(&source)
            .arg("-o")
            .arg(&object))?;
        objects.push(object);
    }
    let linked = work.join(format!("{}.o", name));
    run(command("ld.lld").arg("-r").arg("-o").arg(&linked).args(&objects))?;
    check_symbols(name, &linked, exports)?;
    install_module(dir, name, &linked, modules_dir)?;
    let ids = modules_dir.join(format!("{}.ids", name));
    match output(command("python3").arg(root().join("tools/linuxkpi/device-ids.py")).arg(&linked)) {
        Ok(text) => write(&ids, &text)?,
        Err(_) => {
            if dir.join("module.ids").exists() {
                fs::copy(dir.join("module.ids"), &ids).map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(())
}

fn rust_module(arch: Arch, name: &str, dir: &Path, modules_dir: &Path, exports: &[String]) -> Result {
    let relocation = if arch == Arch::X86_64 { "pie" } else { "static" };
    run(crate::toolchain::cargo()?.current_dir(dir).args([
        "rustc",
        "--profile",
        "module",
        "-Z",
        "build-std=core,compiler_builtins,alloc",
        "-Z",
        "build-std-features=compiler-builtins-mem",
        "--target",
        &arch.kernel_target(),
        "--crate-type",
        "staticlib",
        "--",
        "-C",
        &format!("relocation-model={}", relocation),
    ]))?;
    let krate = name.replace('-', "_");
    let archive = root().join("target").join(arch.kernel_triple()).join("module").join(format!("lib{}.a", krate));
    let work = root().join("target/rmodules").join(arch.name()).join(name);
    let _ = fs::remove_dir_all(&work);
    mkdirs(&work)?;
    let listing = output(command("ar").arg("t").arg(&archive))?;
    let member = listing.lines().find(|l| l.starts_with(&format!("{}-", krate))).ok_or_else(|| format!("{} did not produce a relocatable object", name))?;
    run(command("ar").current_dir(&work).arg("x").arg(&archive).arg(member))?;
    let object = work.join(format!("{}.o", name));
    fs::rename(work.join(member), &object).map_err(|e| e.to_string())?;
    check_symbols(name, &object, exports)?;
    check_relocations(arch, name, &object)?;
    install_module(dir, name, &object, modules_dir)?;
    if dir.join("module.ids").exists() {
        install(dir.join("module.ids"), &modules_dir.join(format!("{}.ids", name)), "644")?;
    }
    Ok(())
}

fn modules(arch: Arch, rootfs: &Path) -> Result {
    say("Building loadable driver modules...");
    let exports = kernel_exports()?;
    let modules_dir = rootfs.join("lib/modules");
    mkdirs(&modules_dir)?;
    for dir in subdirs(&root().join("modules")) {
        let name = file_name(&dir);
        if name.starts_with("example-") {
            continue;
        }
        let is_c = !dir.join("Cargo.toml").exists() && fs::read_dir(dir.join("src")).map(|mut e| e.any(|f| f.map(|f| f.path().extension().map(|x| x == "c").unwrap_or(false)).unwrap_or(false))).unwrap_or(false);
        let result = if is_c { c_module(arch, &name, &dir, &modules_dir, &exports) } else { rust_module(arch, &name, &dir, &modules_dir, &exports) };
        if let Err(e) = result {
            if e.contains("does not export") || e.contains("absolute relocations") {
                return Err(e);
            }
            warn(&format!("module {} does not build for {}, skipping ({})", name, arch.name(), e));
        }
    }
    Ok(())
}

fn overlays(opts: &Options, rootfs: &Path) -> Result {
    say("Copying data/overlay on top of the root filesystem...");
    copy_tree(&root().join("data/overlay"), rootfs)?;
    if opts.profile.linux() {
        say("Copying the Linux layer (layers/linux/overlay) and its environment (layers/linux/root -> /linux)...");
        copy_tree(&root().join("layers/linux/overlay"), rootfs)?;
        copy_tree(&root().join("layers/linux/root"), &rootfs.join("linux"))?;
        for dir in ["etc", "var/lib", "var/cache", "var/log", "usr/local/bin", "usr/share/applications"] {
            mkdirs(&rootfs.join("linux").join(dir))?;
        }
    }
    chmod(&rootfs.join("etc/shadow"), 0o600)?;
    chmod(&rootfs.join("etc/sudoers"), 0o600)?;
    chmod(&rootfs.join("usr/share/hamix/install.sh"), 0o755)?;
    let mut random = [0u8; 16];
    std::io::Read::read_exact(&mut fs::File::open("/dev/urandom").map_err(|e| e.to_string())?, &mut random).map_err(|e| e.to_string())?;
    let id: String = random.iter().map(|b| format!("{:02x}", b)).collect();
    write(&rootfs.join("etc/machine-id"), &format!("{}\n", id))?;
    write(&rootfs.join("var/log/boot.log"), "")?;
    let profile = if opts.profile.linux() { "hamix-full" } else { "hamix-core" };
    write(&rootfs.join("etc/hamix/profile"), &format!("{}\n", profile))?;
    write(&rootfs.join("etc/hamix/desktop"), &format!("{}\n", opts.desktop.name()))?;
    if !opts.desktop.nook() {
        for dir in ["usr/share/nook", "usr/share/wallpapers", "var/cache/nook"] {
            let _ = fs::remove_dir_all(rootfs.join(dir));
        }
        let _ = fs::remove_file(rootfs.join("usr/share/applications/org.hamix.Nook.desktop"));
        let conf = rootfs.join("etc/hamix/login.conf");
        let text = fs::read_to_string(&conf).unwrap_or_default();
        let mut lines: Vec<String> = text.lines().filter(|l| !l.trim_start().starts_with("desktop=")).map(String::from).collect();
        lines.push(String::from("desktop=/usr/bin/enlightenment-session"));
        write(&conf, &(lines.join("\n") + "\n"))?;
    }
    if !opts.profile.linux() {
        write(&rootfs.join("etc/hamix/linux.conf"), "enabled=no\n")?;
    }
    let tests = root().join("tests/boot");
    if tests.is_dir() {
        copy_tree(&tests, &rootfs.join("usr/lib/hamix/tests"))?;
    }
    if let Some(extra) = &opts.extra {
        if extra.is_dir() {
            say(&format!("Copying {} on top of the root filesystem...", extra.display()));
            copy_tree(extra, rootfs)?;
        }
    }
    Ok(())
}

fn mkhext() -> Result<PathBuf> {
    say("Building mkhext (host tool)...");
    let host = host_triple()?;
    run(crate::toolchain::cargo()?.current_dir(root().join("tools/mkhext")).args(["build", "--release", "--target", &host]))?;
    let bin = root().join("target").join(&host).join("release/mkhext");
    if bin.exists() {
        Ok(bin)
    } else {
        Err(String::from("mkhext was not built"))
    }
}

pub fn objcopy() -> Result<Vec<String>> {
    for tool in ["llvm-objcopy", "aarch64-linux-gnu-objcopy"] {
        if have(tool) {
            return Ok(vec![String::from(tool)]);
        }
    }
    let sysroot = output(crate::toolchain::rustc()?.args(["--print", "sysroot"]))?.trim().to_string();
    let host = host_triple()?;
    Ok(vec![String::from("env"), format!("LD_LIBRARY_PATH={}/lib", sysroot), format!("{}/lib/rustlib/{}/bin/rust-objcopy", sysroot, host)])
}

fn port_images(arch: Arch, rootfs: &Path, kernel_bin: &Path, mkhext: &Path) -> Result {
    let out = arch.port_out();
    mkdirs(&out)?;
    say(&format!("Building the {} root image ({}/hext.img)...", arch.name(), out.display()));
    run(command(&mkhext.display().to_string()).args(["--label", "hamix-live"]).arg(rootfs).arg(out.join("hext.img")))?;
    fs::copy(kernel_bin, out.join("kernel.elf")).map_err(|e| e.to_string())?;
    if arch != Arch::Aarch64 {
        say(&format!("Done: {}/kernel.elf + {}/hext.img", out.display(), out.display()));
        return Ok(());
    }
    let oc = objcopy()?;
    run(command(&oc[0]).args(&oc[1..]).args(["-O", "binary"]).arg(kernel_bin).arg(out.join("Image")))?;
    let nm = output(command("nm").arg(kernel_bin))?;
    let size = nm
        .lines()
        .find(|l| l.ends_with(" __efi_file_size"))
        .and_then(|l| u64::from_str_radix(l.split_whitespace().next()?, 16).ok())
        .ok_or("kernel has no __efi_file_size symbol")?;
    fs::OpenOptions::new().write(true).open(out.join("Image")).and_then(|f| f.set_len(size)).map_err(|e| e.to_string())?;
    let esp = out.join("esp/EFI/BOOT");
    let _ = fs::remove_dir_all(out.join("esp"));
    mkdirs(&esp)?;
    fs::copy(out.join("Image"), esp.join("BOOTAA64.EFI")).map_err(|e| e.to_string())?;
    fs::copy(out.join("hext.img"), out.join("esp/hext.img")).map_err(|e| e.to_string())?;
    if have("mkfs.fat") && have("mcopy") {
        let hext_len = fs::metadata(out.join("hext.img")).map_err(|e| e.to_string())?.len();
        let mb = (hext_len + size) / 1_048_576 + 16;
        let efi = out.join("efi.img");
        let _ = fs::remove_file(&efi);
        fs::File::create(&efi).and_then(|f| f.set_len(mb * 1_048_576)).map_err(|e| e.to_string())?;
        run_quiet(command("mkfs.fat").args(["-F", "32", "-n", "HAMIXEFI"]).arg(&efi))?;
        run(command("mmd").arg("-i").arg(&efi).args(["::/EFI", "::/EFI/BOOT"]))?;
        run(command("mcopy").arg("-i").arg(&efi).arg(out.join("Image")).arg("::/EFI/BOOT/BOOTAA64.EFI"))?;
        run(command("mcopy").arg("-i").arg(&efi).arg(out.join("hext.img")).arg("::/hext.img"))?;
    }
    say(&format!("Done: {}/Image (arm64 Image with an EFI stub) + {}/hext.img", out.display(), out.display()));
    Ok(())
}

fn iso(rootfs: &Path, kernel_bin: &Path, mkhext: &Path, profile: Profile) -> Result<PathBuf> {
    let iso_dir = out_dir().join("isoroot");
    mkdirs(&iso_dir.join("boot/grub"))?;
    let _ = fs::remove_file(iso_dir.join("boot/kmods.tar"));
    fs::copy(rootfs.join("boot/initfs.tar"), iso_dir.join("boot/initfs.tar")).map_err(|e| e.to_string())?;
    say("Building the live image (boot/hext.img)...");
    let hext = iso_dir.join("boot/hext.img");
    let _ = fs::remove_file(iso_dir.join("boot/hext.img.gz"));
    run(command(&mkhext.display().to_string()).args(["--label", "hamix-live"]).arg(rootfs).arg(&hext))?;
    let disk = out_dir().join("hamix_disk.img");
    if !disk.exists() {
        say(&format!("Creating a blank 1G test disk for QEMU ({})...", disk.display()));
        fs::File::create(&disk).and_then(|f| f.set_len(1 << 30)).map_err(|e| e.to_string())?;
    }
    let _ = fs::remove_file(iso_dir.join("boot/initramfs.tar"));
    let _ = fs::remove_file(iso_dir.join("boot/disk.img"));
    let mut grub = fs::read_to_string(root().join("data/grub/grub.cfg")).map_err(|e| e.to_string())?;
    if !profile.linux() {
        grub = grub.replace("root=live", "root=live linux=0").replace("root=auto", "root=auto linux=0");
    }
    write(&iso_dir.join("boot/grub/grub.cfg"), &grub)?;
    let mut hxboot = fs::read_to_string(root().join("data/hxboot/hxboot.cfg")).map_err(|e| e.to_string())?;
    if !profile.linux() {
        hxboot = hxboot.replace("root=live", "root=live linux=0").replace("root=auto", "root=auto linux=0").replace("root=ram", "root=ram linux=0");
    }
    write(&iso_dir.join("boot/hxboot.cfg"), &hxboot)?;
    say("Building ISO image...");
    fs::copy(kernel_bin, iso_dir.join("boot/kernel.bin")).map_err(|e| e.to_string())?;
    let name = if profile.linux() { "hamix_os.iso" } else { "hamix_core.iso" };
    let iso = out_dir().join(name);
    crate::bootloader::make_iso(&iso_dir, &iso, "HAMIXOS", crate::bootloader::Loader::current()?)?;
    say(&format!("Done: {}", iso.display()));
    Ok(iso)
}

fn remove_build_only(dir: &Path) -> Result {
    let Ok(entries) = fs::read_dir(dir) else {
        return Ok(());
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_dir() && !path.is_symlink() {
            remove_build_only(&path)?;
        } else if matches!(path.extension().and_then(|e| e.to_str()), Some("a") | Some("la") | Some("o")) {
            fs::remove_file(&path).map_err(|e| format!("{}: {}", path.display(), e))?;
        }
    }
    Ok(())
}

fn remove_ports(arch: Arch, names: &[&str], rootfs: &Path) -> Result {
    let recipes = crate::ports::recipes()?;
    let closure = |roots: Vec<String>| -> Vec<String> {
        let mut seen: Vec<String> = Vec::new();
        let mut stack = roots;
        while let Some(name) = stack.pop() {
            if seen.contains(&name) {
                continue;
            }
            if let Some(r) = recipes.iter().find(|r| r.name == name) {
                stack.extend(r.depends.iter().cloned());
            }
            seen.push(name);
        }
        seen
    };
    let needed_elsewhere = closure(recipes.iter().filter(|r| !names.contains(&r.name.as_str()) && !recipes.iter().any(|o| o.depends.contains(&r.name))).map(|r| r.name.clone()).collect());
    let drop: Vec<String> = closure(names.iter().map(|n| String::from(*n)).collect()).into_iter().filter(|n| !needed_elsewhere.contains(n)).collect();
    let lists = out_dir().join("hx-ports").join(arch.name()).join("files");
    let mut removed = 0;
    for name in &drop {
        for file in fs::read_to_string(lists.join(format!("{}.list", name))).unwrap_or_default().lines().filter(|l| !l.is_empty()) {
            if fs::remove_file(rootfs.join(file)).is_ok() {
                removed += 1;
            }
        }
    }
    say(&format!("Left out for this desktop: {} ({} files)", drop.join(", "), removed));
    Ok(())
}

fn hxlibc_runtime(opts: &Options, rootfs: &Path) -> Result {
    let sysroot = crate::hxlibc::sysroot(opts.arch);
    if !sysroot.join("usr/lib/libhxc.so").exists() {
        if let Err(e) = crate::hxlibc::build(opts.arch) {
            warn(&format!("hxlibc is not available for {}: {}", opts.arch.name(), e));
            return Ok(());
        }
    }
    let ported = crate::ports::stage_root(opts.arch);
    if ported.is_dir() {
        say("Copying programs built by hx-ports (out/hx-ports)...");
        copy_tree(&ported, rootfs)?;
        for dir in ["usr/include", "usr/lib/pkgconfig", "usr/share/pkgconfig", "usr/share/man", "usr/share/doc", "usr/share/aclocal", "usr/share/gtk-doc", "usr/share/gettext", "usr/share/bash-completion", "usr/lib/cmake", "usr/lib/glib-2.0/include", "usr/share/glib-2.0/gdb", "usr/share/gdb", "usr/libexec/installed-tests", "usr/share/installed-tests", "usr/share/xml"] {
            let _ = fs::remove_dir_all(rootfs.join(dir));
        }
        for tool in ["pcre2test", "pcre2grep", "pcre2-config", "cairo-trace", "dbus-test-tool", "xmlwf", "curl-config", "libpng-config", "libpng16-config", "c_rehash", "png-fix-itxt", "pngfix", "gdbus-codegen", "glib-genmarshal", "glib-mkenums", "glib-compile-schemas", "glib-compile-resources", "gtester", "gtester-report", "gobject-query", "glib-gettextize", "gi-compile-repository", "gi-decompile-typelib", "gi-inspect-typelib", "pango-view", "pango-list", "pango-segmentation", "gapplication", "gresource", "gio-querymodules", "fc-validate", "fc-pattern", "fc-query", "fc-scan", "fc-conflist"] {
            let _ = fs::remove_file(rootfs.join("usr/bin").join(tool));
        }
        remove_build_only(&rootfs.join("usr/lib"))?;
        mkdirs(&rootfs.join("usr/share/doc"))?;
        if !opts.desktop.enlightenment() {
            remove_ports(opts.arch, &ENLIGHTENMENT_PORTS, rootfs)?;
        }
    }
    say("Installing hxlibc (ld.hx, libhxc.so) and building the C test programs...");
    install(sysroot.join("usr/lib/ld.hx"), &rootfs.join("lib/ld.hx"), "755")?;
    for entry in fs::read_dir(sysroot.join("usr/lib")).map_err(|e| e.to_string())?.filter_map(|e| e.ok()) {
        let name = file_name(&entry.path());
        if name.ends_with(".so") && name != "libc.so" {
            install(entry.path(), &rootfs.join("usr/lib").join(&name), "755")?;
        }
    }
    let cc = root().join("hxlibc/hx-cc");
    if let Err(e) = crate::hxlibc::host_toolchain() {
        warn(&format!("no freestanding clang headers: {}", e));
    }
    let bench = rootfs.join("usr/lib/hamix/bench");
    mkdirs(&bench)?;
    if let Err(e) = run(command(&cc.display().to_string()).env("HX_ARCH", opts.arch.name()).args(["-O2", "-o"]).arg(bench.join("kbench")).arg(root().join("tools/kbench/kbench.c")).arg("-lpthread")) {
        warn(&format!("kbench does not build: {}", e));
    }
    let musl = root().join("target/musl-root");
    if opts.profile.linux() && opts.arch == Arch::X86_64 && musl.join("usr/lib/libc.a").exists() {
        let result = run(command(&root().join("layers/linux/ports/musl-cc").display().to_string())
            .env("MUSL_ROOT", &musl)
            .args(["-O2", "-o"])
            .arg(bench.join("kbench-musl"))
            .arg(root().join("tools/kbench/kbench.c")));
        if let Err(e) = result {
            warn(&format!("kbench-musl does not build: {}", e));
        }
    }
    let tests = root().join("tests/hxlibc");
    let bin = rootfs.join("usr/lib/hamix/tests/bin");
    mkdirs(&bin)?;
    let mut sources: Vec<PathBuf> = fs::read_dir(&tests).map(|e| e.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().map(|x| x == "c" || x == "cpp").unwrap_or(false)).collect()).unwrap_or_default();
    sources.sort();
    let stage = crate::ports::stage_root(opts.arch);
    for source in sources {
        let name = source.file_stem().unwrap().to_string_lossy().into_owned();
        let packages = fs::read_to_string(source.with_extension("pkg")).unwrap_or_default();
        let packages: Vec<&str> = packages.split_whitespace().collect();
        let mut flags: Vec<String> = Vec::new();
        if !packages.is_empty() {
            let found = output(
                command("pkg-config")
                    .env("PKG_CONFIG_SYSROOT_DIR", &stage)
                    .env("PKG_CONFIG_LIBDIR", format!("{}/usr/lib/pkgconfig:{}/usr/share/pkgconfig", stage.display(), stage.display()))
                    .args(["--static", "--cflags", "--libs"])
                    .args(&packages),
            );
            match found {
                Ok(text) => flags.extend(text.split_whitespace().map(String::from)),
                Err(e) => {
                    warn(&format!("tests/hxlibc/{}.c needs {} (run `cargo xtask ports`): {}", name, packages.join(" "), e));
                    continue;
                }
            }
        }
        let cxx = source.extension().map(|x| x == "cpp").unwrap_or(false);
        let compiler = if cxx { root().join("hxlibc/hx-c++") } else { cc.clone() };
        let standard: &[&str] = if cxx { &["-std=c++20"] } else { &[] };
        let result = run(command(&compiler.display().to_string()).env("HX_ARCH", opts.arch.name()).args(["-O2", "-Wall"]).args(standard).arg("-o").arg(bin.join(&name)).arg(&source).args(&flags).arg("-lpthread"));
        if let Err(e) = result {
            warn(&format!("tests/hxlibc/{} does not build: {}", file_name(&source), e));
        }
    }
    Ok(())
}

fn initfs(rootfs: &Path) -> Result {
    let stage = root().join("target/initfs");
    let _ = fs::remove_dir_all(&stage);
    let modules = stage.join("lib/modules");
    mkdirs(&modules)?;
    let installed = rootfs.join("lib/modules");
    let mut early = Vec::new();
    for entry in fs::read_dir(&installed).map_err(|e| format!("{}: {}", installed.display(), e))?.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.extension().map(|x| x != "conf").unwrap_or(true) {
            continue;
        }
        let conf = fs::read_to_string(&path).unwrap_or_default();
        if !conf.lines().any(|l| l.split('#').next().unwrap_or("").replace(' ', "") == "early=1") {
            continue;
        }
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        for ext in ["ko", "ids", "conf", "sig"] {
            let file = installed.join(format!("{}.{}", stem, ext));
            if file.exists() {
                fs::copy(&file, modules.join(format!("{}.{}", stem, ext))).map_err(|e| e.to_string())?;
            }
        }
        early.push(stem);
    }
    early.sort();
    write(&stage.join("etc/initfs.conf"), &format!("{}\n", early.join("\n")))?;
    say(&format!("Packing the boot drivers (boot/initfs.tar): {}", early.join(", ")));
    let _ = fs::remove_file(rootfs.join("boot/kmods.tar"));
    let tar = crate::util::gnu("tar");
    let format: &[&str] = if cfg!(target_os = "openbsd") && tar == "tar" { &[] } else { &["--format=ustar"] };
    run(command(&tar).args(format).arg("-C").arg(&stage).arg("-cf").arg(rootfs.join("boot/initfs.tar")).arg("lib").arg("etc"))
}

pub fn build(opts: &Options) -> Result {
    let kernel_bin = kernel(opts.arch)?;
    let rootfs = base_rootfs(opts, &kernel_bin)?;
    userland(opts, &rootfs)?;
    hxlibc_runtime(opts, &rootfs)?;
    modules(opts.arch, &rootfs)?;
    overlays(opts, &rootfs)?;
    initfs(&rootfs)?;
    let mkhext = mkhext()?;
    if opts.arch != Arch::X86_64 {
        return port_images(opts.arch, &rootfs, &kernel_bin, &mkhext);
    }
    iso(&rootfs, &kernel_bin, &mkhext, opts.profile)?;
    Ok(())
}
