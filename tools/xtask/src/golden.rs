use std::fs;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use crate::image::{Arch, Profile};
use crate::util::*;

struct Monitor {
    stream: UnixStream,
}

impl Monitor {
    fn connect(path: &Path) -> Result<Monitor> {
        for _ in 0..100 {
            if let Ok(stream) = UnixStream::connect(path) {
                stream.set_read_timeout(Some(Duration::from_millis(200))).map_err(|e| e.to_string())?;
                let mut m = Monitor { stream };
                m.drain();
                return Ok(m);
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        Err(String::from("cannot connect to the QEMU monitor"))
    }

    fn drain(&mut self) {
        let mut buf = [0u8; 4096];
        while let Ok(n) = self.stream.read(&mut buf) {
            if n == 0 {
                break;
            }
        }
    }

    fn cmd(&mut self, text: &str) {
        let _ = self.stream.write_all(format!("{}\n", text).as_bytes());
        std::thread::sleep(Duration::from_millis(40));
        self.drain();
    }

    fn key(&mut self, key: &str) {
        self.cmd(&format!("sendkey {}", key));
        std::thread::sleep(Duration::from_millis(40));
    }

    fn typ(&mut self, text: &str) {
        for c in text.chars() {
            let key = match c {
                ' ' => String::from("spc"),
                '\n' => String::from("ret"),
                '-' => String::from("minus"),
                '/' => String::from("slash"),
                '.' => String::from("dot"),
                c if c.is_ascii_uppercase() => format!("shift-{}", c.to_ascii_lowercase()),
                c => c.to_string(),
            };
            self.key(&key);
        }
    }

    fn pointer(&mut self, x: i32, y: i32) {
        for _ in 0..25 {
            self.cmd("mouse_move -100 -100");
        }
        let (mut x, mut y) = (x, y);
        while x > 0 || y > 0 {
            let (dx, dy) = (x.min(60), y.min(60));
            self.cmd(&format!("mouse_move {} {}", dx, dy));
            std::thread::sleep(Duration::from_millis(30));
            x -= dx;
            y -= dy;
        }
    }

    fn click(&mut self, x: i32, y: i32) {
        self.pointer(x, y);
        std::thread::sleep(Duration::from_millis(300));
        self.cmd("mouse_button 1");
        std::thread::sleep(Duration::from_millis(250));
        self.cmd("mouse_button 0");
    }

    fn shot(&mut self, path: &Path) {
        self.cmd(&format!("screendump {}", path.display()));
        std::thread::sleep(Duration::from_millis(600));
    }
}

fn wait(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

const SCENES: [&str; 5] = ["desktop", "dock-hover", "launcher", "terminal", "app-menu"];

fn capture(dir: &Path) -> Result {
    let iso = crate::qemu::iso_path(Profile::Full);
    let sock = out_dir().join("golden/monitor.sock");
    let _ = fs::remove_file(&sock);
    mkdirs(dir)?;
    let mut cmd = crate::qemu::base_command(Arch::X86_64, "1G")?;
    cmd.arg("-cdrom").arg(&iso).args(["-boot", "d", "-display", "none", "-serial"]).arg(format!("file:{}", out_dir().join("golden/serial.log").display())).arg("-monitor").arg(format!("unix:{},server,nowait", sock.display()));
    cmd.stdout(Stdio::null()).stderr(Stdio::null());
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    let result = (|| {
        let mut m = Monitor::connect(&sock)?;
        let serial = out_dir().join("golden/serial.log");
        for _ in 0..180 {
            if fs::read_to_string(&serial).map(|t| t.contains("org.freedesktop.Accounts")).unwrap_or(false) {
                break;
            }
            wait(500);
        }
        wait(8000);
        m.typ("user\n");
        wait(15000);
        m.pointer(900, 400);
        wait(1500);
        m.shot(&dir.join("desktop.ppm"));
        m.pointer(368, 724);
        wait(1500);
        m.shot(&dir.join("dock-hover.ppm"));
        m.pointer(900, 400);
        wait(500);
        m.key("meta_l");
        wait(2000);
        m.shot(&dir.join("launcher.ppm"));
        m.key("esc");
        wait(1500);
        m.click(368, 724);
        wait(4000);
        m.pointer(900, 650);
        wait(1500);
        m.shot(&dir.join("terminal.ppm"));
        m.click(60, 16);
        wait(1500);
        m.shot(&dir.join("app-menu.ppm"));
        m.cmd("quit");
        Ok(())
    })();
    let _ = child.kill();
    let _ = child.wait();
    result
}

fn python(script: &str, args: &[&Path]) -> Result<String> {
    let mut cmd = command("python3");
    cmd.arg(root().join("tools/nook-golden").join(script));
    for a in args {
        cmd.arg(a);
    }
    output(&mut cmd)
}

pub fn golden(update: bool) -> Result {
    let golden = root().join("tests/nook-golden");
    let fresh = out_dir().join("golden/current");
    let _ = fs::remove_dir_all(&fresh);
    say("golden: booting Nook and capturing scenes...");
    capture(&fresh)?;
    let params = python("params.py", &[&root().join("userland/desktop/hxserver/src"), &root().join("userland/desktop/nook-shell/src")])?;
    if update {
        mkdirs(&golden)?;
        for scene in SCENES {
            python("convert.py", &[&fresh.join(format!("{}.ppm", scene)), &golden.join(format!("{}.png", scene))])?;
        }
        write(&golden.join("parameters.md"), &params)?;
        say(&format!("golden: reference updated in {}", golden.display()));
        return Ok(());
    }
    let mut failed = Vec::new();
    for scene in SCENES {
        let reference: PathBuf = golden.join(format!("{}.png", scene));
        if !reference.exists() {
            failed.push(format!("{}: no reference (run cargo xtask golden --update)", scene));
            continue;
        }
        let diff = fresh.join(format!("{}-diff.png", scene));
        let report = python("compare.py", &[&reference, &fresh.join(format!("{}.ppm", scene)), &diff])?;
        let percent: f64 = report.trim().parse().unwrap_or(100.0);
        println!("  {:<12} {:.3}% of pixels differ", scene, percent);
        if percent > 1.0 {
            failed.push(format!("{}: {:.2}% differ, see {}", scene, percent, diff.display()));
        }
    }
    let committed = fs::read_to_string(golden.join("parameters.md")).unwrap_or_default();
    if committed != params {
        failed.push(String::from("animation/layout parameters changed (tests/nook-golden/parameters.md)"));
    }
    if failed.is_empty() {
        say("golden: Nook matches the reference");
        Ok(())
    } else {
        Err(format!("golden:\n  {}", failed.join("\n  ")))
    }
}
