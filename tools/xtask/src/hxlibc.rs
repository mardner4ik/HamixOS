use std::fs;
use std::path::{Path, PathBuf};

use crate::image::Arch;
use crate::util::*;

const MESON_VERSION: &str = "1.9.1";

pub fn work_dir() -> PathBuf {
    root().join("target/hxlibc")
}

pub fn sysroot(arch: Arch) -> PathBuf {
    out_dir().join("hxlibc").join(arch.name()).join("sysroot")
}

const TOOLS_REVISION: u32 = 2;

pub fn tools() -> Result<PathBuf> {
    let venv = root().join("target/tools/venv");
    let stamp = venv.join(format!("hamix-tools-{}", TOOLS_REVISION));
    if stamp.exists() {
        return Ok(venv.join("bin"));
    }
    say("Preparing the build-tool environment (meson, ninja, mako, PyYAML) in target/tools/venv...");
    if !venv.join("bin/pip").exists() {
        run(command("python3").args(["-m", "venv"]).arg(&venv))?;
    }
    run(command(&venv.join("bin/pip").display().to_string()).args(["install", "-q", &format!("meson=={}", MESON_VERSION), "ninja", "mako", "PyYAML", "packaging"]))?;
    write(&stamp, "")?;
    Ok(venv.join("bin"))
}

fn kernel_headers(arch: Arch) -> Result<PathBuf> {
    let dir = work_dir().join(format!("kernel-headers-{}", arch.name()));
    if dir.join("linux").exists() {
        return Ok(dir);
    }
    let host = Path::new("/usr/include");
    let source = if arch == Arch::X86_64 && host.join("linux/version.h").exists() && host.join("asm/unistd.h").exists() {
        host.to_path_buf()
    } else {
        alpine_headers(arch)?
    };
    mkdirs(&dir)?;
    for sub in ["linux", "asm", "asm-generic"] {
        run(command("cp").arg("-a").arg(source.join(sub)).arg(dir.join(sub)))?;
    }
    Ok(dir)
}

const ALPINE_HEADERS: &str = "https://dl-cdn.alpinelinux.org/alpine/v3.20/main";

fn alpine_headers(arch: Arch) -> Result<PathBuf> {
    let work = work_dir().join(format!("alpine-headers-{}", arch.name()));
    let include = work.join("usr/include");
    if include.join("asm/unistd.h").exists() {
        return Ok(include);
    }
    mkdirs(&work)?;
    say(&format!("Fetching Linux UAPI headers for {} (Alpine linux-headers)...", arch.name()));
    let index = output(command("curl").arg("-fsSL").arg(format!("{}/{}/", ALPINE_HEADERS, arch.name())))?;
    let name = index
        .split('"')
        .find(|s| s.starts_with("linux-headers-") && s.ends_with(".apk"))
        .ok_or_else(|| format!("no linux-headers package for {} in {}", arch.name(), ALPINE_HEADERS))?
        .to_string();
    let apk = work.join(&name);
    run(command("curl").args(["-fsSL", "-o"]).arg(&apk).arg(format!("{}/{}/{}", ALPINE_HEADERS, arch.name(), name)))?;
    let _ = run(command("tar").arg("xzf").arg(&apk).arg("-C").arg(&work).arg("usr/include"));
    if !include.join("asm/unistd.h").exists() {
        return Err(format!("{} has no usr/include/asm/unistd.h", apk.display()));
    }
    Ok(include)
}

pub fn alpine_builtins(arch: Arch) -> Result<PathBuf> {
    let work = work_dir().join(format!("alpine-compiler-rt-{}", arch.name()));
    let lib = work.join("lib");
    let wanted = format!("libclang_rt.builtins-{}.a", arch.name());
    if lib.join(&wanted).exists() {
        return Ok(lib);
    }
    mkdirs(&lib)?;
    say(&format!("Fetching compiler-rt builtins for {} (Alpine compiler-rt)...", arch.name()));
    let index = output(command("curl").arg("-fsSL").arg(format!("{}/{}/", ALPINE_HEADERS, arch.name())))?;
    let name = index
        .split('"')
        .find(|s| s.starts_with("compiler-rt-") && s.ends_with(".apk"))
        .ok_or_else(|| format!("no compiler-rt package for {} in {}", arch.name(), ALPINE_HEADERS))?
        .to_string();
    let apk = work.join(&name);
    run(command("curl").args(["-fsSL", "-o"]).arg(&apk).arg(format!("{}/{}/{}", ALPINE_HEADERS, arch.name(), name)))?;
    let tar = crate::util::gnu("tar");
    let listing = output(command(&tar).arg("tzf").arg(&apk))?;
    let member = listing
        .lines()
        .find(|l| l.ends_with(&format!("/{}", wanted)) || l.ends_with("/libclang_rt.builtins.a"))
        .ok_or_else(|| format!("{} has no {}", apk.display(), wanted))?
        .to_string();
    run(command(&tar).arg("xzf").arg(&apk).arg("-C").arg(&work).arg(&member))?;
    fs::copy(work.join(&member), lib.join(&wanted)).map_err(|e| e.to_string())?;
    fs::copy(work.join(&member), lib.join("libgcc.a")).map_err(|e| e.to_string())?;
    Ok(lib)
}

const ALPINE_CLANG: &str = "https://dl-cdn.alpinelinux.org/alpine/v3.22/main/x86_64";

pub fn host_toolchain() -> Result<Option<PathBuf>> {
    if !have("gcc") {
        let shim = crate::util::out_dir().join("hxlibc/host-bin/gcc");
        write(&shim, "#!/bin/sh\nexec clang --target=${HX_ARCH:-x86_64}-unknown-linux-gnu -fuse-ld=lld \"$@\"\n")?;
        chmod(&shim, 0o755)?;
    }
    let resource = PathBuf::from(output(command("clang").arg("-print-resource-dir"))?.trim()).join("include");
    if resource.join("stddef.h").exists() && resource.join("stdarg.h").exists() {
        return Ok(None);
    }
    let dir = crate::util::out_dir().join("hxlibc/clang-include");
    if dir.join("stddef.h").exists() {
        return Ok(Some(dir));
    }
    let major = output(command("clang").arg("-dumpversion"))?.trim().split('.').next().unwrap_or("").to_string();
    say(&format!("clang {} has no freestanding headers here; fetching them from Alpine (clang{}-headers)...", major, major));
    let index = output(command("curl").arg("-fsSL").arg(format!("{}/", ALPINE_CLANG)))?;
    let prefix = format!("clang{}-headers-", major);
    let name = index
        .split('"')
        .find(|s| s.starts_with(&prefix) && s.ends_with(".apk"))
        .ok_or_else(|| format!("no clang{}-headers package in {}", major, ALPINE_CLANG))?
        .to_string();
    let work = work_dir().join(format!("alpine-clang{}-headers", major));
    let _ = fs::remove_dir_all(&work);
    mkdirs(&work)?;
    let apk = work.join(&name);
    run(command("curl").args(["-fsSL", "-o"]).arg(&apk).arg(format!("{}/{}", ALPINE_CLANG, name)))?;
    let _ = run_quiet(command(&crate::util::gnu("tar")).arg("xzf").arg(&apk).arg("-C").arg(&work));
    let include = work.join(format!("usr/lib/llvm{}/lib/clang/{}/include", major, major));
    if !include.join("stddef.h").exists() {
        return Err(format!("{} has no {}", name, include.display()));
    }
    let _ = fs::remove_dir_all(&dir);
    crate::util::copy_tree(&include, &dir)?;
    Ok(Some(dir))
}

pub fn host_builtins(arch: Arch) -> Option<PathBuf> {
    let text = output(command("clang").arg(format!("--target={}-unknown-linux-gnu", arch.name())).args(["--rtlib=compiler-rt", "-print-libgcc-file-name"])).ok()?;
    let path = PathBuf::from(text.trim());
    path.is_file().then_some(path)
}

fn source() -> Result<PathBuf> {
    let src = root().join("hxlibc");
    if !src.join("meson.build").exists() || !src.join("subprojects/frigg/include").exists() {
        return Err(String::from("hxlibc/ is not a complete mlibc source tree (meson.build, subprojects/frigg)"));
    }
    Ok(src)
}

pub fn build(arch: Arch) -> Result {
    let bin = tools()?;
    let src = source()?;
    let headers = kernel_headers(arch)?;
    let build = work_dir().join(format!("build-{}", arch.name()));
    let _ = fs::remove_dir_all(&build);
    let path = format!("{}:{}:{}", root().join("hxlibc/cross/bin").display(), bin.display(), std::env::var("PATH").unwrap_or_default());
    let builtins = if arch == Arch::X86_64 && host_builtins(arch).is_some() { None } else { Some(alpine_builtins(arch)?) };
    say(&format!("Configuring hxlibc (mlibc + sysdeps/hamix) for {}...", arch.name()));
    let mut setup = command(&bin.join("meson").display().to_string());
    setup
        .env("PATH", &path)
        .current_dir(&src)
        .arg("setup")
        .arg(&build)
        .arg("--cross-file")
        .arg(root().join(format!("hxlibc/cross/hamix-{}.txt", arch.name())))
        .args(["--prefix=/usr", "--libdir=lib", "--buildtype=release", "-Ddefault_library=both", "-Dbuild_tests=false"])
        .arg(format!("-Dlinux_kernel_headers={}", headers.display()));
    if let Some(dir) = &builtins {
        let extra = if arch == Arch::Riscv64 { " -Wl,-z,max-page-size=4096 -nostdlib --target=riscv64-unknown-linux-gnu" } else { " -Wl,-z,max-page-size=4096" };
        setup.arg(format!("-Dc_link_args=-L{}{}", dir.display(), extra)).arg(format!("-Dcpp_link_args=-L{}{}", dir.display(), extra));
    }
    run(&mut setup)?;
    say("Building hxlibc...");
    run(command(&bin.join("ninja").display().to_string()).env("PATH", &path).arg("-C").arg(&build))?;
    let sysroot = sysroot(arch);
    let _ = fs::remove_dir_all(&sysroot);
    run(command(&bin.join("meson").display().to_string()).env("PATH", &path).arg("install").arg("-C").arg(&build).arg("--destdir").arg(&sysroot).arg("--quiet"))?;
    write(&sysroot.join("usr/lib/libc.so"), "GROUP ( libhxc.so )\n")?;
    if let Some(dir) = &builtins {
        for name in ["libgcc.a", &format!("libclang_rt.builtins-{}.a", arch.name())] {
            fs::copy(dir.join(name), sysroot.join("usr/lib").join(name)).map_err(|e| e.to_string())?;
        }
    }
    write(&sysroot.join("usr/include/sys/soundcard.h"), "#include <linux/soundcard.h>\n")?;
    let include = sysroot.join("usr/include");
    for sub in ["linux", "asm", "asm-generic"] {
        mkdirs(&include.join(sub))?;
        run(command(&crate::util::gnu("cp")).arg("-a").arg("--update=none").arg(format!("{}/.", headers.join(sub).display())).arg(include.join(sub)))?;
    }
    say(&format!("hxlibc sysroot ready: {} (compile with hxlibc/hx-cc)", sysroot.display()));
    Ok(())
}
