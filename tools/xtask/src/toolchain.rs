use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

use crate::util::{chmod, command, have, mkdirs, output, root, run, say, write, Result};

struct Toolchain {
    rustup: bool,
    src_root: Option<PathBuf>,
    shims: Option<PathBuf>,
}

static TOOLCHAIN: OnceLock<std::result::Result<Toolchain, String>> = OnceLock::new();

fn toolchain() -> Result<&'static Toolchain> {
    TOOLCHAIN.get_or_init(detect).as_ref().map_err(Clone::clone)
}

fn detect() -> Result<Toolchain> {
    if have("rustup") {
        return Ok(Toolchain { rustup: true, src_root: None, shims: None });
    }
    let sysroot = PathBuf::from(output(command("rustc").args(["--print", "sysroot"]))?.trim());
    let verbose = output(command("rustc").arg("-vV"))?;
    let host = verbose.lines().find_map(|l| l.strip_prefix("host: ")).ok_or_else(|| String::from("rustc -vV has no host line"))?;
    let version = verbose.lines().find_map(|l| l.strip_prefix("release: ")).ok_or_else(|| String::from("rustc -vV has no release line"))?;
    let src_root = if sysroot.join("lib/rustlib/src/rust/library/Cargo.lock").exists() { None } else { Some(rust_src(version)?) };
    let shims = if have("rust-lld") || sysroot.join("lib/rustlib").join(host).join("bin/rust-lld").exists() { None } else { Some(lld_shim()?) };
    Ok(Toolchain { rustup: false, src_root, shims })
}

fn rust_src(version: &str) -> Result<PathBuf> {
    if version.contains('-') {
        return Err(format!("rustc {} has no rust-src in this sysroot; install rust-src or use a released compiler", version));
    }
    let dir = root().join("target/rust-src").join(version);
    let library = dir.join(format!("rust-src-{}/rust-src/lib/rustlib/src/rust/library", version));
    if library.join("Cargo.lock").exists() {
        return Ok(library);
    }
    mkdirs(&dir)?;
    let archive = dir.join("rust-src.tar.xz");
    let url = format!("https://static.rust-lang.org/dist/rust-src-{}.tar.xz", version);
    say(&format!("Downloading the standard library sources for rustc {} (build-std)...", version));
    if have("curl") {
        run(command("curl").args(["-fsSL", "-o"]).arg(&archive).arg(&url))?;
    } else {
        run(command("ftp").arg("-o").arg(&archive).arg(&url))?;
    }
    run(command("sh").args(["-c", "xz -dc \"$1\" | tar -xf - -C \"$2\"", "sh"]).arg(&archive).arg(&dir))?;
    let _ = std::fs::remove_file(&archive);
    if library.join("Cargo.lock").exists() {
        Ok(library)
    } else {
        Err(format!("{} has no Cargo.lock after unpacking rust-src", library.display()))
    }
}

fn lld_shim() -> Result<PathBuf> {
    if !have("ld.lld") {
        return Err(String::from("neither rust-lld nor ld.lld is installed"));
    }
    let defaults = if cfg!(target_os = "openbsd") { " -no-pie --no-execute-only" } else { "" };
    let dir = root().join("target/host-shims");
    let shim = dir.join("rust-lld");
    write(&shim, &format!("#!/bin/sh\nif [ \"$1\" = \"-flavor\" ]; then shift 2; fi\nexec ld.lld{} \"$@\"\n", defaults))?;
    chmod(&shim, 0o755)?;
    Ok(dir)
}

fn tool(program: &str) -> Result<Command> {
    let toolchain = toolchain()?;
    let mut cmd = command(program);
    if toolchain.rustup {
        cmd.arg("+nightly");
        return Ok(cmd);
    }
    cmd.env("RUSTC_BOOTSTRAP", "1");
    if let Some(src) = &toolchain.src_root {
        cmd.env("__CARGO_TESTS_ONLY_SRC_ROOT", src);
    }
    if let Some(shims) = &toolchain.shims {
        let path = std::env::var_os("PATH").unwrap_or_default();
        let mut dirs = vec![shims.clone()];
        dirs.extend(std::env::split_paths(&path));
        cmd.env("PATH", std::env::join_paths(dirs).map_err(|e| e.to_string())?);
    }
    if cfg!(target_os = "openbsd") && std::env::var_os("MALLOC_OPTIONS").is_none() {
        cmd.env("MALLOC_OPTIONS", "j");
    }
    Ok(cmd)
}

pub fn cargo() -> Result<Command> {
    tool("cargo")
}

pub fn rustc() -> Result<Command> {
    tool("rustc")
}
