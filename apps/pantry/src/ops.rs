use alloc::collections::BTreeSet;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cmp::Ordering;
use hamix_std::{fs, println, sys};

use crate::archive::{self, Kind};
use crate::crypto;
use crate::db::{self, Db, Installed, LINK_MAGIC, SYSROOT};
use crate::http;
use crate::index::{compare_versions, parse_dep, provided_name, Dep, Pkg};
use crate::repo::Catalog;
use crate::scripts;

const PROTECTED: [&str; 7] = ["etc/passwd", "etc/shadow", "etc/group", "etc/hostname", "etc/hosts", "etc/resolv.conf", "etc/machine-id"];
const RECOMMENDS: [(&str, &str, &str); 6] = [
    ("fontconfig", "font-", "font-dejavu"),
    ("xterm", "font-misc-misc", "font-misc-misc"),
    ("libreoffice-common", "libreoffice-gtk", "libreoffice-gtk"),
    ("qt6-qtbase", "qt6-qtwayland", "qt6-qtwayland"),
    ("qt5-qtbase", "qt5-qtwayland", "qt5-qtwayland"),
    ("gtk+3.0", "adwaita-icon-theme", "adwaita-icon-theme"),
];

pub fn human(bytes: u64) -> String {
    if bytes >= 1 << 20 {
        format!("{}.{} MiB", bytes >> 20, ((bytes % (1 << 20)) * 10) >> 20)
    } else {
        format!("{} KiB", bytes.div_ceil(1024))
    }
}

fn better(cat: &Catalog, a: usize, b: usize) -> bool {
    let (pa, pb) = (&cat.pkgs[a], &cat.pkgs[b]);
    match compare_versions(&pa.version, &pb.version) {
        Ordering::Greater => true,
        Ordering::Less => false,
        Ordering::Equal => pa.repo < pb.repo,
    }
}

pub fn newest(cat: &Catalog, name: &str) -> Option<usize> {
    let mut best: Option<usize> = None;
    for &i in cat.by_name.get(name)? {
        if best.map(|b| better(cat, i, b)).unwrap_or(true) {
            best = Some(i);
        }
    }
    best
}

fn provides_dep(pkg_name: &str, version: &str, provides: &[String], dep: &Dep) -> bool {
    if pkg_name == dep.name && dep.satisfied_by(version) {
        return true;
    }
    provides.iter().any(|token| {
        let (name, pv) = provided_name(token);
        name == dep.name && (dep.op.is_none() || pv.map(|v| dep.satisfied_by(v)).unwrap_or(false))
    })
}

pub fn provider(cat: &Catalog, dep: &Dep) -> Option<usize> {
    let mut best: Option<usize> = None;
    for &i in cat.by_name.get(&dep.name).map(|v| v.as_slice()).unwrap_or(&[]) {
        if dep.satisfied_by(&cat.pkgs[i].version) && best.map(|b| better(cat, i, b)).unwrap_or(true) {
            best = Some(i);
        }
    }
    if best.is_some() {
        return best;
    }
    for &i in cat.by_provide.get(&dep.name).map(|v| v.as_slice()).unwrap_or(&[]) {
        let p = &cat.pkgs[i];
        if !provides_dep(&p.name, &p.version, &p.provides, dep) {
            continue;
        }
        let wins = match best {
            None => true,
            Some(b) => {
                let q = &cat.pkgs[b];
                p.priority > q.priority || (p.priority == q.priority && better(cat, i, b))
            }
        };
        if wins {
            best = Some(i);
        }
    }
    best
}

fn installed_satisfies(db: &Db, dep: &Dep) -> bool {
    db.packages.iter().any(|p| provides_dep(&p.name, &p.version, &p.provides, dep))
}

struct Solver<'a> {
    cat: &'a Catalog,
    db: &'a Db,
    order: Vec<usize>,
    planned: BTreeSet<usize>,
    visiting: BTreeSet<usize>,
    replace: BTreeSet<String>,
}

impl<'a> Solver<'a> {
    fn planned_satisfies(&self, dep: &Dep) -> bool {
        self.planned.iter().any(|&i| {
            let p = &self.cat.pkgs[i];
            provides_dep(&p.name, &p.version, &p.provides, dep)
        })
    }

    fn want(&mut self, token: &str, needed_by: &str) -> Result<(), String> {
        let dep = parse_dep(token);
        if dep.conflict {
            return Ok(());
        }
        if self.planned_satisfies(&dep) {
            return Ok(());
        }
        let replacing = self.replace.contains(&dep.name);
        if !replacing && installed_satisfies(self.db, &dep) {
            return Ok(());
        }
        let Some(index) = provider(self.cat, &dep) else {
            return Err(if needed_by.is_empty() { format!("no package named '{}'", token) } else { format!("nothing provides '{}' (needed by {})", token, needed_by) });
        };
        self.add(index)
    }

    fn add(&mut self, index: usize) -> Result<(), String> {
        if self.planned.contains(&index) || self.visiting.contains(&index) {
            return Ok(());
        }
        let pkg = &self.cat.pkgs[index];
        if let Some(current) = self.db.find(&pkg.name) {
            if current.is_system() || (current.version == pkg.version && !self.replace.contains(&pkg.name)) {
                return Ok(());
            }
        }
        self.visiting.insert(index);
        let name = pkg.name.clone();
        for dep in pkg.depends.clone() {
            self.want(&dep, &name)?;
        }
        self.visiting.remove(&index);
        self.planned.insert(index);
        self.order.push(index);
        Ok(())
    }

    fn add_install_if(&mut self) -> Result<(), String> {
        loop {
            let mut added = false;
            for (i, pkg) in self.cat.pkgs.iter().enumerate() {
                if pkg.install_if.is_empty() || self.planned.contains(&i) || self.db.find(&pkg.name).is_some() {
                    continue;
                }
                if newest(self.cat, &pkg.name) != Some(i) {
                    continue;
                }
                let triggered = pkg.install_if.iter().all(|t| {
                    let dep = parse_dep(t);
                    installed_satisfies(self.db, &dep) || self.planned_satisfies(&dep)
                });
                let touches_plan = pkg.install_if.iter().any(|t| self.planned_satisfies(&parse_dep(t)));
                if triggered && touches_plan {
                    self.add(i)?;
                    added = true;
                }
            }
            if !added {
                return Ok(());
            }
        }
    }
}

pub fn plan(cat: &Catalog, db: &Db, targets: &[String], replace: BTreeSet<String>) -> Result<Vec<usize>, String> {
    let mut solver = Solver { cat, db, order: Vec::new(), planned: BTreeSet::new(), visiting: BTreeSet::new(), replace };
    for target in targets {
        let dep = parse_dep(target);
        match provider(cat, &dep) {
            Some(i) => solver.add(i)?,
            None => return Err(format!("no package named '{}' (try 'pantry search {}')", target, dep.name)),
        }
    }
    solver.add_install_if()?;
    for (trigger, family, recommended) in RECOMMENDS {
        let wanted = solver.order.iter().any(|&i| cat.pkgs[i].name == trigger);
        let present = db.packages.iter().any(|p| p.name.starts_with(family)) || solver.order.iter().any(|&i| cat.pkgs[i].name.starts_with(family));
        if wanted && !present && solver.want(recommended, "").is_ok() {
            println!("Adding {} for {}", recommended, trigger);
        }
    }
    let needs_x = solver.order.iter().any(|&i| {
        let deps = &cat.pkgs[i].depends;
        let x11 = deps.iter().any(|d| X11_LIBS.iter().any(|l| d.starts_with(l)));
        let wayland = deps.iter().any(|d| WAYLAND_LIBS.iter().any(|l| d.starts_with(l)));
        x11 && !wayland
    });
    let has_x = db.packages.iter().any(|p| p.name == "xwayland") || solver.order.iter().any(|&i| cat.pkgs[i].name == "xwayland");
    if needs_x && !has_x && solver.want("xwayland", "").is_ok() {
        println!("Adding xwayland so that X11 programs can open windows on the Nook desktop");
    }
    let needs_bus = solver.order.iter().any(|&i| {
        cat.pkgs[i].depends.iter().any(|d| d.starts_with("so:libdbus-1.") || d.starts_with("so:libgtk-3.") || d.starts_with("so:libgtk-4."))
    });
    let has_bus = db.packages.iter().any(|p| p.name == "dbus") || solver.order.iter().any(|&i| cat.pkgs[i].name == "dbus");
    if needs_bus && !has_bus && solver.want("dbus", "").is_ok() {
        let _ = solver.want("dbus-x11", "");
        println!("Adding dbus so desktop apps get a session bus on the Nook desktop");
    }
    solver.add_install_if()?;
    Ok(solver.order)
}

const X11_LIBS: [&str; 2] = ["so:libX11.so", "so:libxcb.so"];
const WAYLAND_LIBS: [&str; 5] = ["so:libwayland-client.so", "so:libgtk-3.so", "so:libgdk-3.so", "so:libgtk-4.so", "so:libSDL2-2.0.so"];

fn is_script(name: &str) -> bool {
    name.starts_with('.') && (name.ends_with("-install") || name.ends_with("-upgrade") || name.ends_with("-deinstall") || name == ".trigger")
}

fn target_path(rel: &str) -> String {
    format!("{}/{}", SYSROOT, rel)
}

pub fn fetch_verified(cat: &Catalog, pkg: &Pkg) -> Result<Vec<u8>, String> {
    let repo = &cat.repos[pkg.repo];
    let url = repo.package_url(pkg);
    http::get(&url, &pkg.name, false)
}

fn virtual_names(provides: &[String], own: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for token in provides {
        let (name, _) = provided_name(token);
        if name != own && !out.iter().any(|n| n == name) {
            out.push(name.to_string());
        }
    }
    out
}

fn exclusive_names(provides: &[String], own: &str) -> Vec<String> {
    virtual_names(provides, own).into_iter().filter(|n| !n.contains(':')).collect()
}

fn displaced_by(db: &Db, name: &str, provides: &[String]) -> Vec<String> {
    let mine = exclusive_names(provides, name);
    if mine.is_empty() {
        return Vec::new();
    }
    db.packages
        .iter()
        .filter(|p| !p.is_system() && p.name != name)
        .filter(|p| exclusive_names(&p.provides, &p.name).iter().any(|n| mine.contains(n)))
        .map(|p| p.name.clone())
        .collect()
}

fn displace(db: &mut Db, name: &str, replacement: &str, changed: &mut BTreeSet<String>) {
    let Some(pkg) = db.remove(name) else {
        return;
    };
    println!("  replacing {} {} with {}", pkg.name, pkg.version, replacement);
    scripts::run(&pkg.name, ".pre-deinstall", &[&pkg.version]);
    for file in pkg.files.iter().rev() {
        sys::unlink(&target_path(file));
        changed.insert(scripts::changed_dir(file));
    }
    scripts::run(&pkg.name, ".post-deinstall", &[&pkg.version]);
    scripts::forget(&pkg.name);
    db.world.retain(|w| w != name);
}

fn may_take_over(replaces: &[String], pkg: &Pkg, owner: &Installed) -> bool {
    if replaces.iter().any(|token| {
        let dep = parse_dep(token);
        !dep.conflict && provides_dep(&owner.name, &owner.version, &owner.provides, &dep)
    }) {
        return true;
    }
    let mine = virtual_names(&pkg.provides, &pkg.name);
    virtual_names(&owner.provides, &owner.name).iter().any(|n| mine.contains(n))
}

pub fn install_one(cat: &Catalog, db: &mut Db, index: usize, changed: &mut BTreeSet<String>) -> Result<(), String> {
    let pkg = &cat.pkgs[index];
    for stale in displaced_by(db, &pkg.name, &pkg.provides) {
        displace(db, &stale, &pkg.name, changed);
    }
    let previous = db.find(&pkg.name).cloned();
    match &previous {
        Some(old) => println!("Upgrading {} {} -> {} ({})", pkg.name, old.version, pkg.version, human(pkg.size)),
        None => println!("Installing {} {} ({})", pkg.name, pkg.version, human(pkg.size)),
    }
    let bytes = fetch_verified(cat, pkg)?;
    let raws = archive::gzip_raw_members(&bytes, 3).map_err(|e| format!("{}: {}", pkg.name, e))?;
    let wanted = pkg.checksum.strip_prefix("Q1").and_then(crypto::base64_decode).ok_or_else(|| format!("{}: index has no usable checksum", pkg.name))?;
    let control_at = raws.iter().position(|m| crypto::sha1(m).as_slice() == wanted.as_slice()).ok_or_else(|| format!("{}: checksum mismatch, refusing to install", pkg.name))?;
    if control_at + 1 >= raws.len() {
        return Err(format!("{}: package has no data section", pkg.name));
    }
    let control_members = archive::gzip_members(raws[control_at], 1).map_err(|e| format!("{}: {}", pkg.name, e))?;
    let control_data = control_members.first().map(|m| m.data.clone()).ok_or_else(|| format!("{}: package has no control section", pkg.name))?;
    let control = archive::tar_entries(&control_data).map_err(|e| format!("{}: {}", pkg.name, e))?;
    let pkginfo = control.iter().find(|e| e.path == ".PKGINFO").map(|e| String::from_utf8_lossy(e.data).into_owned()).ok_or_else(|| format!("{}: missing .PKGINFO", pkg.name))?;
    let datahash = pkginfo.lines().find_map(|l| l.strip_prefix("datahash = ")).map(|s| s.trim().to_string());
    let data_raw = &bytes[raws[..=control_at].iter().map(|m| m.len()).sum::<usize>()..];
    match datahash {
        Some(expected) => {
            if crypto::hex(&crypto::sha256(data_raw)) != expected {
                return Err(format!("{}: data hash mismatch, refusing to install", pkg.name));
            }
        }
        None => return Err(format!("{}: package has no data hash, refusing to install", pkg.name)),
    }
    let scripts: Vec<String> = control.iter().filter(|e| is_script(&e.path)).map(|e| e.path.clone()).collect();
    let script_bodies: Vec<(String, Vec<u8>)> = control.iter().filter(|e| is_script(&e.path)).map(|e| (e.path.clone(), e.data.to_vec())).collect();
    let triggers: Vec<String> = pkginfo.lines().filter_map(|l| l.strip_prefix("triggers = ")).flat_map(|t| t.split_whitespace().map(String::from)).collect();
    let replaces: Vec<String> = pkginfo.lines().filter_map(|l| l.strip_prefix("replaces = ")).flat_map(|t| t.split_whitespace().map(String::from)).collect();

    let mut listing: Vec<(String, Kind)> = Vec::new();
    {
        let mut tar = archive::TarStream::new();
        let mut collect = |event: archive::TarEvent| {
            if let archive::TarEvent::Begin(entry) = event {
                listing.push((entry.path.clone(), entry.kind.clone()));
            }
            true
        };
        let mut sink = |chunk: &[u8]| tar.feed(chunk, &mut collect);
        archive::gzip_stream(data_raw, &mut sink).map_err(|e| format!("{}: {}", pkg.name, e))?;
        if let Some(e) = tar.error {
            return Err(format!("{}: {}", pkg.name, e));
        }
    }
    let mut taken: Vec<(String, String)> = Vec::new();
    for (path, kind) in &listing {
        if *kind == Kind::Dir || *kind == Kind::Other {
            continue;
        }
        if let Some(owner) = db.owner_of(path) {
            if owner.name != pkg.name {
                if !may_take_over(&replaces, pkg, owner) {
                    return Err(format!("{}: file /{} already belongs to {}", pkg.name, path, owner.name));
                }
                taken.push((owner.name.clone(), path.clone()));
            }
        }
    }
    if !taken.is_empty() {
        let mut donors: Vec<String> = Vec::new();
        for (owner, path) in &taken {
            if let Some(old) = db.packages.iter_mut().find(|p| &p.name == owner) {
                old.files.retain(|f| f != path);
            }
            if !donors.contains(owner) {
                donors.push(owner.clone());
            }
        }
        println!("  taking over {} file(s) from {}", taken.len(), donors.join(", "));
    }

    scripts::store(&pkg.name, &script_bodies);
    let old_version = previous.as_ref().map(|p| p.version.clone());
    match &old_version {
        Some(old) => scripts::run(&pkg.name, ".pre-upgrade", &[&pkg.version, old]),
        None => scripts::run(&pkg.name, ".pre-install", &[&pkg.version]),
    };
    let mut files: Vec<String> = Vec::new();
    let mut skipped_protected = Vec::new();
    let mut failure: Option<String> = None;
    {
        let mut open: Option<(i64, String, u32)> = None;
        let mut tar = archive::TarStream::new();
        let mut write = |event: archive::TarEvent| -> bool {
            match event {
                archive::TarEvent::Begin(entry) => {
                    let dest = target_path(&entry.path);
                    match &entry.kind {
                        Kind::Dir => {
                            db::mkdir_p(&dest);
                            changed.insert(scripts::changed_dir(entry.path.trim_end_matches('/')));
                            changed.insert(format!("/{}", entry.path.trim_matches('/')));
                        }
                        Kind::File => {
                            if PROTECTED.contains(&entry.path.as_str()) {
                                skipped_protected.push(entry.path.clone());
                                return true;
                            }
                            db::mkdir_p(db::parent(&dest));
                            let fd = sys::open_with(&dest, sys::O_WRONLY | sys::O_CREAT | sys::O_TRUNC);
                            if fd < 0 {
                                failure = Some(format!("{}: cannot write {}", pkg.name, dest));
                                return false;
                            }
                            open = Some((fd, dest, entry.mode.max(0o400)));
                            files.push(entry.path.clone());
                        }
                        Kind::Symlink(link) => {
                            let mut body = Vec::with_capacity(LINK_MAGIC.len() + link.len());
                            body.extend_from_slice(LINK_MAGIC);
                            body.extend_from_slice(link.as_bytes());
                            if !db::write_file(&dest, &body) {
                                failure = Some(format!("{}: cannot create link {}", pkg.name, dest));
                                return false;
                            }
                            sys::chmod(&dest, 0o777);
                            files.push(entry.path.clone());
                        }
                        Kind::Hardlink(target) => {
                            let Some(data) = fs::read(&target_path(target)) else {
                                failure = Some(format!("{}: hard link target {} missing", pkg.name, target));
                                return false;
                            };
                            if !db::write_file(&dest, &data) {
                                failure = Some(format!("{}: cannot write {}", pkg.name, dest));
                                return false;
                            }
                            sys::chmod(&dest, entry.mode.max(0o400));
                            files.push(entry.path.clone());
                        }
                        Kind::Other => {}
                    }
                }
                archive::TarEvent::Data(chunk) => {
                    if let Some((fd, dest, _)) = &open {
                        let mut done = 0;
                        while done < chunk.len() {
                            let n = sys::write(*fd as u64, &chunk[done..]);
                            if n <= 0 {
                                failure = Some(format!("{}: cannot write {} (disk full?)", pkg.name, dest));
                                return false;
                            }
                            done += n as usize;
                        }
                    }
                }
                archive::TarEvent::End => {
                    if let Some((fd, dest, mode)) = open.take() {
                        sys::close(fd as u64);
                        sys::chmod(&dest, mode);
                    }
                }
            }
            true
        };
        let mut sink = |chunk: &[u8]| tar.feed(chunk, &mut write);
        let streamed = archive::gzip_stream(data_raw, &mut sink);
        if let Some((fd, _, _)) = open.take() {
            sys::close(fd as u64);
        }
        if let Some(e) = failure.take() {
            return Err(e);
        }
        streamed.map_err(|e| format!("{}: {}", pkg.name, e))?;
        if let Some(e) = tar.error {
            return Err(format!("{}: {}", pkg.name, e));
        }
    }
    if let Some(old) = &previous {
        for stale in old.files.iter().filter(|f| !files.contains(f)) {
            sys::unlink(&target_path(stale));
            changed.insert(scripts::changed_dir(stale));
        }
    }
    for file in files.iter() {
        changed.insert(scripts::changed_dir(file));
    }
    if !skipped_protected.is_empty() {
        println!("  kept HamixOS versions of /{}", skipped_protected.join(", /"));
    }
    match &old_version {
        Some(old) => scripts::run(&pkg.name, ".post-upgrade", &[&pkg.version, old]),
        None => scripts::run(&pkg.name, ".post-install", &[&pkg.version]),
    };
    db.remove(&pkg.name);
    db.packages.push(Installed {
        name: pkg.name.clone(),
        version: pkg.version.clone(),
        repo: cat.repos[pkg.repo].name.clone(),
        description: pkg.description.clone(),
        depends: pkg.depends.clone(),
        provides: pkg.provides.clone(),
        files,
        scripts,
        triggers,
    });
    if !db.save() {
        return Err(String::from("cannot save the package database"));
    }
    Ok(())
}

pub fn run_plan(cat: &Catalog, db: &mut Db, order: &[usize]) -> Result<(), String> {
    let download: u64 = order.iter().map(|i| cat.pkgs[*i].size).sum();
    let installed: u64 = order.iter().map(|i| cat.pkgs[*i].installed_size).sum();
    println!("{} package(s), {} to download, {} on disk:", order.len(), human(download), human(installed));
    let names: Vec<String> = order.iter().map(|i| format!("{}-{}", cat.pkgs[*i].name, cat.pkgs[*i].version)).collect();
    println!("  {}", names.join(" "));
    let mut changed = BTreeSet::new();
    let mut result = Ok(());
    for (n, &i) in order.iter().enumerate() {
        hamix_std::print!("({}/{}) ", n + 1, order.len());
        if let Err(e) = install_one(cat, db, i, &mut changed) {
            result = Err(e);
            break;
        }
    }
    scripts::run_triggers(db, &changed);
    result
}

fn needed_by(db: &Db, removing: &BTreeSet<String>) -> Option<(String, String)> {
    for pkg in db.packages.iter().filter(|p| !removing.contains(&p.name)) {
        for token in &pkg.depends {
            let dep = parse_dep(token);
            if dep.conflict {
                continue;
            }
            let still = db.packages.iter().filter(|q| !removing.contains(&q.name)).any(|q| provides_dep(&q.name, &q.version, &q.provides, &dep));
            if !still {
                let by = db.packages.iter().find(|q| removing.contains(&q.name) && provides_dep(&q.name, &q.version, &q.provides, &dep)).map(|q| q.name.clone()).unwrap_or_default();
                if !by.is_empty() {
                    return Some((by, pkg.name.clone()));
                }
            }
        }
    }
    None
}

pub fn orphans(db: &Db, removing: &BTreeSet<String>) -> BTreeSet<String> {
    let mut set = removing.clone();
    loop {
        let mut grew = false;
        for pkg in &db.packages {
            if set.contains(&pkg.name) || db.world.contains(&pkg.name) || pkg.is_system() {
                continue;
            }
            let wanted = db.packages.iter().filter(|q| !set.contains(&q.name) && q.name != pkg.name).any(|q| {
                q.depends.iter().any(|t| {
                    let dep = parse_dep(t);
                    !dep.conflict && provides_dep(&pkg.name, &pkg.version, &pkg.provides, &dep)
                })
            });
            if !wanted {
                set.insert(pkg.name.clone());
                grew = true;
            }
        }
        if !grew {
            return set;
        }
    }
}

pub fn remove(db: &mut Db, names: &[String]) -> Result<(), String> {
    let mut removing: BTreeSet<String> = BTreeSet::new();
    for name in names {
        match db.find(name) {
            None => return Err(format!("{} is not installed", name)),
            Some(p) if p.is_system() => return Err(format!("{} is part of HamixOS and cannot be removed", name)),
            Some(_) => {}
        }
        removing.insert(name.clone());
    }
    if let Some((what, by)) = needed_by(db, &removing) {
        return Err(format!("cannot remove {}: {} needs it", what, by));
    }
    db.world.retain(|w| !removing.contains(w));
    let all = orphans(db, &removing);
    let extra: Vec<String> = all.difference(&removing).cloned().collect();
    if !extra.is_empty() {
        println!("Also removing packages nothing needs any more: {}", extra.join(" "));
    }
    let mut changed = BTreeSet::new();
    for name in &all {
        if let Some(pkg) = db.remove(name) {
            println!("Removing {} {}", pkg.name, pkg.version);
            scripts::run(&pkg.name, ".pre-deinstall", &[&pkg.version]);
            for file in pkg.files.iter().rev() {
                sys::unlink(&target_path(file));
                changed.insert(scripts::changed_dir(file));
            }
            scripts::run(&pkg.name, ".post-deinstall", &[&pkg.version]);
            scripts::forget(&pkg.name);
        }
    }
    scripts::run_triggers(db, &changed);
    if db.save() { Ok(()) } else { Err(String::from("cannot save the package database")) }
}

pub fn upgrades(cat: &Catalog, db: &Db) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    for pkg in db.packages.iter().filter(|p| !p.is_system()) {
        if let Some(i) = newest(cat, &pkg.name) {
            if compare_versions(&cat.pkgs[i].version, &pkg.version) == Ordering::Greater {
                out.push((pkg.name.clone(), pkg.version.clone(), cat.pkgs[i].version.clone()));
            }
        }
    }
    out
}

pub fn owner(db: &Db, path: &str) -> Option<String> {
    let rel = path.trim_start_matches(SYSROOT).trim_start_matches('/').to_string();
    db.owner_of(&rel).map(|p| p.name.clone())
}
