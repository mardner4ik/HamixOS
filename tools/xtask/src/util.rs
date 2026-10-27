use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub type Result<T = ()> = std::result::Result<T, String>;

pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("repository root")
}

pub fn out_dir() -> PathBuf {
    root().join("out")
}

pub fn say(text: &str) {
    println!("[HamixOS] {}", text);
}

pub fn warn(text: &str) {
    println!("[WARN] {}", text);
}

pub fn command(program: &str) -> Command {
    let mut cmd = Command::new(program);
    for key in ["CARGO_MANIFEST_DIR", "CARGO_PKG_NAME", "CARGO_BIN_NAME", "CARGO_CRATE_NAME", "CARGO_PRIMARY_PACKAGE"] {
        cmd.env_remove(key);
    }
    cmd
}

pub fn run(cmd: &mut Command) -> Result {
    let status = cmd.status().map_err(|e| format!("{:?}: {}", cmd.get_program(), e))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{:?} {:?} failed ({})", cmd.get_program(), cmd.get_args().collect::<Vec<_>>(), status))
    }
}

pub fn run_quiet(cmd: &mut Command) -> Result {
    run(cmd.stdout(Stdio::null()).stderr(Stdio::null()))
}

pub fn output(cmd: &mut Command) -> Result<String> {
    let out = cmd.stderr(Stdio::inherit()).output().map_err(|e| format!("{:?}: {}", cmd.get_program(), e))?;
    if !out.status.success() {
        return Err(format!("{:?} failed ({})", cmd.get_program(), out.status));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn have(program: &str) -> bool {
    Command::new("sh").arg("-c").arg(format!("command -v {} >/dev/null 2>&1", program)).status().map(|s| s.success()).unwrap_or(false)
}

pub fn write(path: &Path, text: &str) -> Result {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {}", parent.display(), e))?;
    }
    fs::write(path, text).map_err(|e| format!("{}: {}", path.display(), e))
}

pub fn mkdirs(path: &Path) -> Result {
    fs::create_dir_all(path).map_err(|e| format!("{}: {}", path.display(), e))
}

pub fn chmod(path: &Path, mode: u32) -> Result {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(|e| format!("{}: {}", path.display(), e))
}

pub fn copy_tree(from: &Path, to: &Path) -> Result {
    mkdirs(to)?;
    run(command("cp").arg("-a").arg(format!("{}/.", from.display())).arg(to))
}

pub fn subdirs(path: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = fs::read_dir(path)
        .map(|entries| entries.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()).collect())
        .unwrap_or_default();
    dirs.sort();
    dirs
}

pub fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

pub fn host_triple() -> Result<String> {
    let text = output(crate::toolchain::rustc()?.arg("-vV"))?;
    text.lines().find_map(|l| l.strip_prefix("host: ")).map(String::from).ok_or_else(|| String::from("rustc -vV has no host line"))
}

pub fn install(from: impl AsRef<Path>, to: &Path, mode: &str) -> Result {
    let from = from.as_ref();
    if let Some(parent) = to.parent() {
        mkdirs(parent)?;
    }
    let _ = fs::remove_file(to);
    fs::copy(from, to).map_err(|e| format!("{} -> {}: {}", from.display(), to.display(), e))?;
    chmod(to, u32::from_str_radix(mode, 8).map_err(|e| format!("mode {}: {}", mode, e))?)
}

pub fn gnu(tool: &str) -> String {
    let prefixed = format!("g{}", tool);
    if !cfg!(target_os = "linux") && have(&prefixed) { prefixed } else { String::from(tool) }
}

pub fn sha256(path: &Path) -> Result<String> {
    let text = if have("sha256sum") || have("gsha256sum") {
        output(command(&gnu("sha256sum")).arg(path))?
    } else {
        output(command("sha256").arg("-q").arg(path))?
    };
    text.split_whitespace().next().map(String::from).ok_or_else(|| format!("{}: no sha256", path.display()))
}

pub fn grub_mkrescue(iso: &Path, dir: &Path, extra: &[&str]) -> Result {
    let scratch = out_dir().join("tmp/grub");
    let _ = fs::remove_dir_all(&scratch);
    mkdirs(&scratch)?;
    let result = run_quiet(command("grub-mkrescue").env("TMPDIR", &scratch).arg("-o").arg(iso).arg(dir).args(extra).args(["--", "-volid", "HAMIXOS"]));
    let _ = fs::remove_dir_all(&scratch);
    result
}
