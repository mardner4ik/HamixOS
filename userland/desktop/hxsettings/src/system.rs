use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use hxstd::{env, fs, sys, users};
use hxtoolkit::Pixmap;
use vellum::{Area, Image, Painter};

pub const SYSCONF: &str = "/usr/libexec/hamix/hxsysconf";
pub const THUMB_W: i32 = 160;
pub const THUMB_H: i32 = 100;

pub fn home() -> String {
    env::var("HOME").map(String::from).unwrap_or_else(|| String::from("/"))
}

pub fn user() -> String {
    users::name_of(sys::getuid() as u32)
}

fn nook_conf() -> String {
    format!("{}/.config/nook.conf", home())
}

pub fn nook_config() -> Vec<(String, String)> {
    fs::read_to_string(&nook_conf())
        .map(|t| t.lines().filter_map(|l| l.split_once('=').map(|(k, v)| (String::from(k.trim()), String::from(v.trim())))).collect())
        .unwrap_or_default()
}

pub fn nook_value(key: &str) -> Option<String> {
    nook_config().into_iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

pub fn set_nook_value(key: &str, value: &str) -> bool {
    sys::mkdir(&format!("{}/.config", home()));
    let mut entries = nook_config();
    entries.retain(|(k, _)| k != key);
    entries.push((String::from(key), String::from(value)));
    let mut text = String::new();
    for (k, v) in entries {
        text.push_str(&format!("{}={}\n", k, v));
    }
    fs::write(&nook_conf(), text.as_bytes())
}

fn read_pid(path: &str) -> Option<i64> {
    let pid: i64 = fs::read_to_string(path)?.trim().parse().ok()?;
    sys::proc_alive(pid).then_some(pid)
}

pub fn desktop_pid() -> Option<i64> {
    let runtime = env::var("XDG_RUNTIME_DIR").filter(|d| !d.is_empty()).map(String::from).unwrap_or_else(|| format!("/tmp/runtime-{}", sys::geteuid()));
    read_pid(&format!("{}/hamix-services/hxserver", runtime)).or_else(|| read_pid("/run/hamix/services/hxserver"))
}

pub fn reload_desktop() -> bool {
    desktop_pid().map(|pid| sys::send_signal(pid, sys::SIGHUP) >= 0).unwrap_or(false)
}

pub fn set_desktop(key: &str, value: &str) -> bool {
    let saved = set_nook_value(key, value);
    reload_desktop();
    saved
}

pub fn privileged_without_password() -> bool {
    sys::geteuid() == 0 || sys::access(SYSCONF, sys::X_OK)
}

fn drain(fd: u64) -> String {
    let mut out = Vec::new();
    let mut buf = [0u8; 512];
    loop {
        let n = sys::read(fd, &mut buf);
        if n <= 0 {
            break;
        }
        out.extend_from_slice(&buf[..n as usize]);
    }
    sys::close(fd);
    String::from(String::from_utf8_lossy(&out).trim())
}

pub enum Failure {
    NeedPassword,
    Failed(String),
}

pub fn sysconf(args: &[&str], password: &str) -> Result<(), Failure> {
    let direct = privileged_without_password();
    if !direct && password.is_empty() {
        return Err(Failure::NeedPassword);
    }
    let Ok((err_read, err_write)) = sys::pipe() else {
        return Err(Failure::Failed(String::from("cannot start the helper")));
    };
    let pid = if direct {
        sys::spawn_io(SYSCONF, args, 0, None, Some(err_write), Some(err_write))
    } else {
        let Ok((in_read, in_write)) = sys::pipe() else {
            return Err(Failure::Failed(String::from("cannot start the helper")));
        };
        let mut argv: Vec<&str> = alloc::vec!["-S", "--", SYSCONF];
        argv.extend_from_slice(args);
        let pid = sys::spawn_io("/usr/bin/sudo", &argv, 0, Some(in_read), Some(err_write), Some(err_write));
        sys::close(in_read);
        sys::write(in_write, format!("{}\n", password).as_bytes());
        sys::close(in_write);
        pid
    };
    sys::close(err_write);
    if pid < 0 {
        sys::close(err_read);
        return Err(Failure::Failed(String::from("cannot start the helper")));
    }
    let code = sys::waitpid(pid, false);
    let output = drain(err_read);
    if code == 0 {
        return Ok(());
    }
    let line = output.lines().filter(|l| !l.trim().is_empty() && !l.contains("assword")).last().map(String::from);
    Err(Failure::Failed(match (direct, line) {
        (_, Some(line)) => line,
        (false, None) => String::from("Wrong password, or this account is not an administrator"),
        (true, None) => format!("The change failed (code {})", code),
    }))
}

pub struct Wallpaper {
    pub path: String,
    pub title: String,
    pub credit: String,
    pub thumb: Option<Pixmap>,
}

fn credits() -> Vec<(String, String, String, String)> {
    fs::read_to_string("/usr/share/wallpapers/CREDITS")
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| {
            let f: Vec<&str> = l.split('|').collect();
            (f.len() >= 4).then(|| (String::from(f[0]), String::from(f[1]), String::from(f[2]), String::from(f[3])))
        })
        .collect()
}

pub fn wallpapers() -> Vec<Wallpaper> {
    let mut paths: Vec<String> = Vec::new();
    for dir in [String::from("/usr/share/wallpapers"), format!("{}/Pictures", home())] {
        let mut names: Vec<String> = sys::read_dir(&dir).unwrap_or_default().into_iter().filter(|e| !e.is_dir && imago::extension_format(&e.name).is_some()).map(|e| e.name).collect();
        names.sort();
        paths.extend(names.into_iter().map(|n| format!("{}/{}", dir, n)));
    }
    let credits = credits();
    paths
        .into_iter()
        .map(|path| {
            let file = String::from(path.rsplit('/').next().unwrap_or(""));
            let known = credits.iter().find(|c| c.0 == file && path.starts_with("/usr/share/wallpapers/"));
            let stem = file.rsplit_once('.').map(|(s, _)| s).unwrap_or(&file);
            let title = known.map(|c| c.1.clone()).unwrap_or_else(|| String::from(stem));
            let credit = known.map(|c| if c.2 == "HamixOS project" { String::from("HamixOS") } else { format!("{} · {}", c.2, c.3) }).unwrap_or_else(|| String::from("Your picture"));
            Wallpaper { path, title, credit, thumb: None }
        })
        .collect()
}

fn cover(img: &Image, tw: i32, th: i32) -> Image {
    let wide = img.w as i64 * th as i64 > img.h as i64 * tw as i64;
    let (cw, ch) = if wide { ((img.h as i64 * tw as i64 / th as i64) as i32, img.h) } else { (img.w, (img.w as i64 * th as i64 / tw as i64) as i32) };
    let (ox, oy) = ((img.w - cw) / 2, (img.h - ch) / 2);
    let mut crop = Image { w: cw, h: ch, px: Vec::with_capacity((cw * ch) as usize) };
    for y in 0..ch {
        let start = ((oy + y) * img.w + ox) as usize;
        crop.px.extend_from_slice(&img.px[start..start + cw as usize]);
    }
    crop.scaled(tw, th)
}

pub fn thumbnail(path: &str) -> Pixmap {
    let (dir, file) = path.rsplit_once('/').unwrap_or(("", path));
    let image = fs::read(&format!("{}/thumbs/{}", dir, file)).and_then(|b| Image::decode(&b)).or_else(|| fs::read(path).and_then(|b| Image::decode_sized(&b, THUMB_W * 2, THUMB_H * 2)));
    match image {
        Some(img) if img.w > 0 && img.h > 0 => {
            let small = cover(&img, THUMB_W, THUMB_H);
            Pixmap::from_argb(THUMB_W as u32, THUMB_H as u32, small.px.iter().map(|p| p | 0xff00_0000).collect()).unwrap_or_else(|| Pixmap::new(THUMB_W as u32, THUMB_H as u32))
        }
        _ => Pixmap::filled(THUMB_W as u32, THUMB_H as u32, hxtoolkit::Color::rgb(0x33, 0x33, 0x33)),
    }
}

pub fn login_conf() -> Vec<(String, String)> {
    fs::read_to_string("/etc/hamix/login.conf")
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter_map(|l| l.split_once('=').map(|(k, v)| (String::from(k.trim()), String::from(v.trim()))))
        .collect()
}

pub fn autostart() -> bool {
    login_conf().iter().any(|(k, v)| k == "autostart_desktop" && matches!(v.as_str(), "yes" | "true" | "on" | "1"))
}

pub fn linux_state() -> String {
    let (_, info) = sys::abi_info();
    info.lines().find_map(|l| l.strip_prefix("linux ")).map(|v| String::from(v.trim())).unwrap_or_else(|| String::from("off"))
}

pub fn linux_processes() -> String {
    let (_, info) = sys::abi_info();
    info.lines().find_map(|l| l.strip_prefix("linux_processes")).map(|v| String::from(v.trim())).unwrap_or_default()
}

pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 { format!("{} B", bytes) } else { format!("{:.1} {}", value, UNITS[unit]) }
}

pub fn app_icon(icon: &str) -> String {
    let base = icon.rsplit('/').next().unwrap_or(icon).trim_end_matches(".png").trim_end_matches(".svg");
    if icon.starts_with('/') && icon.ends_with(".png") && fs::exists(icon) {
        return String::from(icon);
    }
    if !base.is_empty() && fs::exists(&format!("/usr/share/nook/icons/apps/{}.png", base)) {
        return format!("apps/{}", base);
    }
    for root in ["", "/linux"] {
        for size in ["48x48", "64x64", "32x32", "128x128"] {
            let path = format!("{}/usr/share/icons/hicolor/{}/apps/{}.png", root, size, base);
            if fs::exists(&path) {
                return path;
            }
        }
    }
    String::from("apps/linux-app")
}

pub fn style_preview(light: bool) -> Pixmap {
    let (w, h) = (THUMB_W, THUMB_H);
    let (desk, panel, window, bar, text, dim, accent) = if light {
        (0xdfe5ee, 0xf6f7f9, 0xffffff, 0xeef0f3, 0x1d2129, 0x8b92a1, 0x2f6fed)
    } else {
        (0x2a3142, 0x15171c, 0x23262d, 0x1b1d22, 0xe8eaee, 0x6b7280, 0x5b8cff)
    };
    let mut px = alloc::vec![0u32; (w * h) as usize];
    {
        let mut p = Painter::new(&mut px, w, h);
        p.fill(Area::new(0, 0, w, h), desk);
        p.fill(Area::new(0, 0, w, 8), panel);
        let win = Area::new(20, 20, w - 40, h - 34);
        p.rounded(win, 6, window, 255);
        p.fill(Area::new(win.x, win.y + 6, win.w, 10), bar);
        p.rounded(Area::new(win.x, win.y, win.w, 16), 6, bar, 255);
        p.rounded(Area::new(win.x + 10, win.y + 26, win.w - 50, 5), 2, text, 220);
        p.rounded(Area::new(win.x + 10, win.y + 36, win.w - 70, 5), 2, dim, 200);
        p.rounded(Area::new(win.right() - 34, win.bottom() - 14, 24, 8), 3, accent, 255);
        p.rounded(Area::new(w / 2 - 24, h - 10, 48, 7), 3, panel, 255);
    }
    Pixmap::from_argb(w as u32, h as u32, px.iter().map(|p| p | 0xff00_0000).collect()).unwrap_or_else(|| Pixmap::new(w as u32, h as u32))
}

pub fn other_users() -> Vec<users::User> {
    let me = sys::getuid() as u32;
    users::users("").into_iter().filter(|u| u.uid >= 1000 && u.uid < 60000 && u.uid != me).collect()
}

pub fn is_admin(name: &str) -> bool {
    let membership = users::member_of("", name);
    users::ADMIN_GROUPS.iter().all(|g| membership.iter().any(|m| m == g))
}
