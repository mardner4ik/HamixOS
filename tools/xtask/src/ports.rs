use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::image::Arch;
use crate::util::*;

pub struct Recipe {
    pub name: String,
    pub version: String,
    pub source: String,
    pub sha256: String,
    pub builddir: String,
    pub depends: Vec<String>,
    pub extract: Vec<String>,
    pub dir: PathBuf,
}

fn field(text: &str, key: &str) -> String {
    text.lines()
        .find_map(|l| l.trim().strip_prefix(&format!("{}=", key)))
        .map(|v| v.trim().trim_matches('"').to_string())
        .unwrap_or_default()
}

fn expand(value: &str, name: &str, version: &str) -> String {
    value.replace("$pkgver", version).replace("${pkgver}", version).replace("$pkgname", name).replace("${pkgname}", name)
}

pub fn recipes() -> Result<Vec<Recipe>> {
    let mut out = Vec::new();
    for dir in crate::workspace::recipe_dirs() {
        let file = dir.join("HXBUILD");
        let Ok(text) = fs::read_to_string(&file) else {
            continue;
        };
        let name = field(&text, "pkgname");
        let version = field(&text, "pkgver");
        if name.is_empty() || version.is_empty() {
            return Err(format!("{}: pkgname and pkgver are required", file.display()));
        }
        out.push(Recipe {
            source: expand(&field(&text, "source"), &name, &version),
            sha256: field(&text, "sha256"),
            builddir: expand(&field(&text, "builddir"), &name, &version),
            depends: field(&text, "depends").split_whitespace().map(String::from).collect(),
            extract: expand(&field(&text, "extract"), &name, &version).split_whitespace().map(String::from).collect(),
            name,
            version,
            dir,
        });
    }
    Ok(out)
}

fn order(all: &[Recipe], wanted: &[String]) -> Result<Vec<usize>> {
    let mut result = Vec::new();
    let mut state = vec![0u8; all.len()];
    fn visit(i: usize, all: &[Recipe], state: &mut Vec<u8>, result: &mut Vec<usize>) -> Result {
        match state[i] {
            2 => return Ok(()),
            1 => return Err(format!("dependency cycle at {}", all[i].name)),
            _ => {}
        }
        state[i] = 1;
        for dep in &all[i].depends {
            let j = all.iter().position(|r| &r.name == dep).ok_or_else(|| format!("{} depends on {}, which has no recipe", all[i].name, dep))?;
            visit(j, all, state, result)?;
        }
        state[i] = 2;
        result.push(i);
        Ok(())
    }
    let targets: Vec<usize> = if wanted.is_empty() {
        (0..all.len()).collect()
    } else {
        wanted.iter().map(|w| all.iter().position(|r| &r.name == w).ok_or_else(|| format!("no recipe called {}", w))).collect::<Result<_>>()?
    };
    for t in targets {
        visit(t, all, &mut state, &mut result)?;
    }
    Ok(result)
}

fn fetch(recipe: &Recipe, cache: &Path) -> Result<Vec<PathBuf>> {
    let sums: Vec<&str> = recipe.sha256.split_whitespace().collect();
    let mut files = Vec::new();
    for (i, entry) in recipe.source.split_whitespace().enumerate() {
        let (name, url) = match entry.split_once("::") {
            Some((name, url)) => (name, url),
            None => (entry.rsplit('/').next().unwrap_or("source"), entry),
        };
        let file = cache.join(name);
        if !file.exists() {
            say(&format!("ports: fetching {}", url));
            mkdirs(cache)?;
            let partial = file.with_extension("part");
            run(command("curl").args(["-fsSL", "-o"]).arg(&partial).arg(url))?;
            fs::rename(&partial, &file).map_err(|e| e.to_string())?;
        }
        if let Some(expected) = sums.get(i) {
            if crate::util::sha256(&file)? != *expected {
                return Err(format!("{}: sha256 mismatch", file.display()));
            }
        }
        files.push(file);
    }
    Ok(files)
}

fn meson_cross(arch: Arch, stage: &Path) -> Result<PathBuf> {
    let dir = root().join("target/hx-ports").join(arch.name());
    mkdirs(&dir)?;
    let file = dir.join("meson-cross.txt");
    let cc = root().join("hxlibc/hx-cc");
    let cxx = root().join("hxlibc/hx-c++");
    let tool = |name: &str| {
        let staged = stage.join("usr/bin").join(name);
        if staged.exists() { staged.display().to_string() } else { name.to_string() }
    };
    let text = format!(
        "[binaries]\nglib-mkenums = '{mkenums}'\nglib-genmarshal = '{genmarshal}'\ngdbus-codegen = '{codegen}'\nglib-compile-resources = 'glib-compile-resources'\nglib-compile-schemas = 'glib-compile-schemas'\nc = ['{cc}']\ncpp = ['{cxx}']\nar = 'llvm-ar'\nstrip = 'llvm-strip'\nnm = 'llvm-nm'\nranlib = 'llvm-ranlib'\npkg-config = 'pkg-config'\n\n[properties]\nsys_root = '{stage}'\npkg_config_libdir = '{stage}/usr/lib/pkgconfig:{stage}/usr/share/pkgconfig'\nneeds_exe_wrapper = true\n\n[built-in options]\nc_args = ['-I{stage}/usr/include', '-D_GNU_SOURCE']\ncpp_args = ['-I{stage}/usr/include', '-D_GNU_SOURCE']\nc_link_args = ['-L{stage}/usr/lib']\ncpp_link_args = ['-L{stage}/usr/lib']\n\n[host_machine]\nsystem = 'linux'\ncpu_family = '{arch}'\ncpu = '{arch}'\nendian = 'little'\n",
        mkenums = tool("glib-mkenums"),
        genmarshal = tool("glib-genmarshal"),
        codegen = tool("gdbus-codegen"),
        cc = cc.display(),
        cxx = cxx.display(),
        stage = stage.display(),
        arch = arch.name()
    );
    write(&file, &text)?;
    Ok(file)
}

fn build_one(arch: Arch, recipe: &Recipe, stage: &Path) -> Result<PathBuf> {
    let work = root().join("target/hx-ports").join(arch.name()).join(&recipe.name);
    let package_only = std::env::var("HX_PORTS_PACKAGE_ONLY").map(|v| v == "1").unwrap_or(false) && work.join("src").is_dir();
    if package_only {
        let _ = fs::remove_dir_all(work.join("pkg"));
    } else {
        let _ = fs::remove_dir_all(&work);
    }
    let srcdir = work.join("src");
    let pkgdir = work.join("pkg");
    mkdirs(&srcdir)?;
    mkdirs(&pkgdir)?;
    let log = work.join("build.log");
    if !recipe.source.is_empty() && !package_only {
        let archives = fetch(recipe, &root().join("target/hx-ports/cache"))?;
        for (i, archive) in archives.iter().enumerate() {
            let only: &[String] = if i == 0 { &recipe.extract } else { &[] };
            run(command("tar").arg("-xf").arg(archive).arg("-C").arg(&srcdir).args(only))?;
        }
    }
    let builddir = if recipe.builddir.is_empty() { srcdir.clone() } else { srcdir.join(&recipe.builddir) };
    let patches = recipe.dir.join("patches");
    if patches.is_dir() && !package_only {
        let mut list: Vec<PathBuf> = fs::read_dir(&patches).map_err(|e| e.to_string())?.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        list.sort();
        for p in list {
            run(command("patch").arg("-s").arg("-p1").arg("-d").arg(&builddir).arg("-i").arg(&p))?;
        }
    }
    let cc = root().join("hxlibc/hx-cc");
    let cxx = root().join("hxlibc/hx-c++");
    let jobs = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2).to_string();
    let cross = meson_cross(arch, stage)?;
    let tools = crate::hxlibc::tools()?;
    let path = format!("{}:{}", tools.display(), std::env::var("PATH").unwrap_or_default());
    let steps = if package_only { "package" } else { "build\npackage" };
    let script = format!("set -e\n. '{}'\ncd '{}'\n{}\n", recipe.dir.join("HXBUILD").display(), builddir.display(), steps);
    let log_file = if package_only { fs::OpenOptions::new().append(true).create(true).open(&log) } else { fs::File::create(&log) }.map_err(|e| e.to_string())?;
    let log_err = log_file.try_clone().map_err(|e| e.to_string())?;
    let status = command("sh")
        .arg("-c")
        .arg(&script)
        .env("CC", &cc)
        .env("CXX", &cxx)
        .env("AR", "llvm-ar")
        .env("RANLIB", "llvm-ranlib")
        .env("STRIP", "llvm-strip")
        .env("NM", "llvm-nm")
        .env("HX_ARCH", arch.name())
        .env("HX_HOST", format!("{}-linux-musl", arch.name()))
        .env("HX_BUILD", "x86_64-pc-linux-gnu")
        .env("HX_ROOT", stage)
        .env("CFLAGS", format!("-O2 -I{}/usr/include", stage.display()))
        .env("CPPFLAGS", format!("-I{}/usr/include", stage.display()))
        .env("CXXFLAGS", format!("-O2 -I{}/usr/include", stage.display()))
        .env("LDFLAGS", format!("-L{}/usr/lib", stage.display()))
        .env("PKG_CONFIG_SYSROOT_DIR", stage)
        .env("PKG_CONFIG_LIBDIR", format!("{}/usr/lib/pkgconfig:{}/usr/share/pkgconfig", stage.display(), stage.display()))
        .env("JOBS", &jobs)
        .env("PATH", &path)
        .env("HX_MESON_CROSS", &cross)
        .env("HX_MESON", format!("--cross-file={} --prefix=/usr --libdir=lib --buildtype=release --wrap-mode=nodownload", cross.display()))
        .env("pkgdir", &pkgdir)
        .env("srcdir", &srcdir)
        .stdout(log_file)
        .stderr(log_err)
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        let text = fs::read_to_string(&log).unwrap_or_default();
        let tail: Vec<&str> = text.lines().rev().take(12).collect::<Vec<_>>().into_iter().rev().collect();
        return Err(format!("build failed, log {}\n    {}", log.display(), tail.join("\n    ")));
    }
    Ok(pkgdir)
}

pub fn stage_root(arch: Arch) -> PathBuf {
    out_dir().join("hx-ports").join(arch.name()).join("root")
}

fn status_path(arch: Arch) -> PathBuf {
    out_dir().join("hx-ports").join(arch.name()).join("status")
}

fn load_status(arch: Arch) -> BTreeMap<String, (String, String)> {
    fs::read_to_string(status_path(arch))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let mut f = l.splitn(3, '\t');
            Some((f.next()?.to_string(), (f.next()?.to_string(), f.next().unwrap_or("").to_string())))
        })
        .collect()
}

fn write_report(arch: Arch, all: &[Recipe], status: &BTreeMap<String, (String, String)>) -> Result {
    let mut text = String::from("# hxlibc: що збирається\n\nStatus: generated by `cargo xtask ports` · не редагувати вручну\n\n");
    text.push_str(&format!("Архітектура: {}. Рецепти: `ports/*/*/HXBUILD`.\n\n| Пакунок | Версія | Стан | Примітка |\n|---|---|---|---|\n", arch.name()));
    let mut ok = 0;
    for r in all {
        let (state, note) = status.get(&r.name).cloned().unwrap_or((String::from("not built"), String::new()));
        if state == "ok" {
            ok += 1;
        }
        text.push_str(&format!("| {} | {} | {} | {} |\n", r.name, r.version, state, note.replace('|', "/")));
    }
    text.push_str(&format!("\nЗібрано: {} з {}.\n", ok, all.len()));
    write(&root().join("docs/reference/hxlibc-status.md"), &text)
}

fn recipe_hash(recipe: &Recipe) -> String {
    let script = format!("cat '{}' '{}'/*.patch 2>/dev/null | sha256sum", recipe.dir.join("HXBUILD").display(), recipe.dir.join("patches").display());
    output(command("sh").arg("-c").arg(&script)).map(|t| t.split_whitespace().next().unwrap_or("").to_string()).unwrap_or_default()
}

fn package_files(dir: &Path) -> Vec<String> {
    let text = output(command("find").arg(".").args(["-not", "-type", "d"]).current_dir(dir)).unwrap_or_default();
    let mut files: Vec<String> = text.lines().map(|l| l.trim_start_matches("./").to_string()).filter(|l| !l.is_empty()).collect();
    files.sort();
    files
}

fn replace_files(arch: Arch, name: &str, pkgdir: &Path, stage: &Path) -> Result {
    let lists = out_dir().join("hx-ports").join(arch.name()).join("files");
    mkdirs(&lists)?;
    let list = lists.join(format!("{}.list", name));
    let fresh = package_files(pkgdir);
    for old in fs::read_to_string(&list).unwrap_or_default().lines() {
        if !old.is_empty() {
            let _ = fs::remove_file(stage.join(old));
        }
    }
    for new in fresh.iter() {
        let _ = fs::remove_file(stage.join(new));
    }
    write(&list, &(fresh.join("\n") + "\n"))
}

fn built_path(arch: Arch) -> PathBuf {
    out_dir().join("hx-ports").join(arch.name()).join("built")
}

pub fn build(arch: Arch, wanted: &[String]) -> Result {
    crate::hxlibc::host_toolchain()?;
    if !crate::hxlibc::sysroot(arch).join("usr/lib/libhxc.so").exists() {
        crate::hxlibc::build(arch)?;
    }
    let all = recipes()?;
    let stage = stage_root(arch);
    mkdirs(&stage)?;
    let mut status = load_status(arch);
    let mut built: BTreeMap<String, String> = fs::read_to_string(built_path(arch))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.split_once('\t').map(|(k, v)| (k.to_string(), v.to_string())))
        .collect();
    let mut failed = Vec::new();
    let mut rebuilt: Vec<String> = Vec::new();
    for i in order(&all, wanted)? {
        let recipe = &all[i];
        let hash = recipe_hash(recipe);
        let requested = wanted.is_empty() || wanted.contains(&recipe.name);
        let deps_rebuilt = recipe.depends.iter().any(|d| rebuilt.contains(d));
        if !requested && !deps_rebuilt && built.get(&recipe.name) == Some(&hash) && status.get(&recipe.name).map(|s| s.0 == "ok").unwrap_or(false) {
            continue;
        }
        if recipe.depends.iter().any(|d| failed.contains(d)) {
            status.insert(recipe.name.clone(), (String::from("blocked"), format!("needs {}", recipe.depends.join(" "))));
            failed.push(recipe.name.clone());
            continue;
        }
        say(&format!("ports: building {} {} for {}", recipe.name, recipe.version, arch.name()));
        match build_one(arch, recipe, &stage) {
            Ok(pkgdir) => {
                let _ = run(command("find").arg(&pkgdir).args(["-name", "*.la", "-delete"]));
                replace_files(arch, &recipe.name, &pkgdir, &stage)?;
                copy_tree(&pkgdir, &stage)?;
                status.insert(recipe.name.clone(), (String::from("ok"), String::new()));
                built.insert(recipe.name.clone(), hash);
                rebuilt.push(recipe.name.clone());
                println!("  ok");
            }
            Err(e) => {
                println!("  FAILED: {}", e);
                let first = e.lines().nth(1).unwrap_or("").trim().to_string();
                status.insert(recipe.name.clone(), (String::from("fails"), first));
                failed.push(recipe.name.clone());
            }
        }
    }
    let text: String = status.iter().map(|(k, (s, n))| format!("{}\t{}\t{}\n", k, s, n)).collect();
    write(&status_path(arch), &text)?;
    let text: String = built.iter().map(|(k, h)| format!("{}\t{}\n", k, h)).collect();
    write(&built_path(arch), &text)?;
    write_report(arch, &all, &status)?;
    if failed.is_empty() {
        Ok(())
    } else {
        Err(format!("ports that did not build: {}", failed.join(", ")))
    }
}
