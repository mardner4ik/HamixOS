#![no_std]
#![no_main]

extern crate alloc;

mod archive;
mod crypto;
mod db;
mod http;
mod index;
mod ops;
mod repo;
mod scripts;

use alloc::collections::BTreeSet;
use alloc::string::String;
use alloc::vec::Vec;
use hamix_std::{entry, env, eprintln, println, sys};

use crate::db::Db;
use crate::repo::Catalog;

const VERSION: &str = "0.1.0";

fn usage() -> i32 {
    println!("pantry {} - the HamixOS package manager", VERSION);
    println!();
    println!("usage: pantry <command> [arguments]");
    println!();
    println!("  update                 download fresh package lists");
    println!("  search <words>         find packages by name or description");
    println!("  info <package>         show details about a package");
    println!("  install <package>...   install packages and what they need");
    println!("  remove <package>...    remove packages and unused dependencies");
    println!("  upgrade                upgrade every installed package");
    println!("  repair                 download and reinstall every installed package");
    println!("  triggers               re-run the post-install triggers (icon, pixbuf, mime caches)");
    println!("  list [--available]     list installed (or all available) packages");
    println!("  files <package>        list the files of an installed package");
    println!("  owner <path>           find which package installed a file");
    println!("  repos                  show the configured repositories");
    println!();
    println!("Linux packages come from Alpine Linux and are installed under {}.", db::SYSROOT);
    2
}

fn require_root() -> bool {
    if sys::geteuid() != 0 {
        eprintln!("pantry: this needs root, try 'sudo pantry ...'");
        return false;
    }
    true
}

fn catalog() -> Option<Catalog> {
    let repos = match repo::load_repos() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("pantry: {}", e);
            return None;
        }
    };
    match repo::load_catalog(repos) {
        Ok(c) => Some(c),
        Err(e) => {
            eprintln!("pantry: {}", e);
            None
        }
    }
}

fn cmd_update() -> i32 {
    if !require_root() {
        return 1;
    }
    let repos = match repo::load_repos() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("pantry: {}", e);
            return 1;
        }
    };
    let keys = repo::load_keys();
    if keys.is_empty() {
        eprintln!("pantry: no trusted keys in {}", db::KEYS_DIR);
        return 1;
    }
    println!("Updating package lists ({} trusted keys)", keys.len());
    if !repo::update(&repos, &keys) {
        return 1;
    }
    if let Some(cat) = catalog() {
        let db = Db::load();
        let pending = ops::upgrades(&cat, &db);
        if !pending.is_empty() {
            println!("{} upgrade(s) available, run 'pantry upgrade'", pending.len());
        }
    }
    0
}

fn cmd_search(words: &[String]) -> i32 {
    if words.is_empty() {
        eprintln!("usage: pantry search <words>");
        return 2;
    }
    let Some(cat) = catalog() else {
        return 1;
    };
    let db = Db::load();
    let needles: Vec<String> = words.iter().map(|w| w.to_lowercase()).collect();
    let mut hits: Vec<usize> = Vec::new();
    for (name, indices) in cat.by_name.iter() {
        let Some(best) = ops::newest(&cat, name) else {
            continue;
        };
        let pkg = &cat.pkgs[best];
        let haystack = alloc::format!("{} {}", pkg.name, pkg.description).to_lowercase();
        if needles.iter().all(|n| haystack.contains(n.as_str())) && !indices.is_empty() {
            hits.push(best);
        }
    }
    hits.sort_by_key(|i| {
        let name = &cat.pkgs[*i].name;
        (!needles.iter().any(|n| name == n), !needles.iter().any(|n| name.starts_with(n.as_str())), name.len())
    });
    for i in hits.iter().take(60) {
        let pkg = &cat.pkgs[*i];
        let mark = if db.find(&pkg.name).is_some() { " [installed]" } else { "" };
        println!("{} {}{}\n    {}", pkg.name, pkg.version, mark, pkg.description);
    }
    if hits.len() > 60 {
        println!("... and {} more, refine the search", hits.len() - 60);
    }
    if hits.is_empty() {
        println!("nothing found");
        return 1;
    }
    0
}

fn cmd_info(name: &str) -> i32 {
    let db = Db::load();
    let cat = catalog();
    let available = cat.as_ref().and_then(|c| ops::newest(c, name).map(|i| (c, i)));
    let installed = db.find(name);
    if available.is_none() && installed.is_none() {
        eprintln!("pantry: no package named '{}'", name);
        return 1;
    }
    if let Some((c, i)) = available {
        let pkg = &c.pkgs[i];
        println!("{} {}", pkg.name, pkg.version);
        println!("  {}", pkg.description);
        println!("  homepage:   {}", pkg.url);
        println!("  license:    {}", pkg.license);
        println!("  repository: {}", c.repos[pkg.repo].name);
        println!("  download:   {}", ops::human(pkg.size));
        println!("  installed:  {}", ops::human(pkg.installed_size));
        if !pkg.depends.is_empty() {
            println!("  depends:    {}", pkg.depends.join(" "));
        }
        if !pkg.provides.is_empty() {
            println!("  provides:   {}", pkg.provides.join(" "));
        }
    }
    match installed {
        Some(p) => {
            let why = if db.world.contains(&p.name) { "you asked for it" } else { "needed by another package" };
            println!("  status:     installed {} ({}), {} files", p.version, why, p.files.len());
            if !p.scripts.is_empty() {
                println!("  scripts:    {}", p.scripts.join(" "));
            }
        }
        None => println!("  status:     not installed"),
    }
    0
}

fn cmd_install(names: &[String]) -> i32 {
    if names.is_empty() {
        eprintln!("usage: pantry install <package>...");
        return 2;
    }
    if !require_root() {
        return 1;
    }
    let Some(cat) = catalog() else {
        return 1;
    };
    let mut db = Db::load();
    let order = match ops::plan(&cat, &db, names, BTreeSet::new()) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("pantry: {}", e);
            return 1;
        }
    };
    for name in names {
        let base = index::parse_dep(name).name;
        if !db.world.contains(&base) {
            db.world.push(base);
        }
    }
    if order.is_empty() {
        db.save();
        println!("Everything is already installed.");
        return 0;
    }
    let result = ops::run_plan(&cat, &mut db, &order);
    match result {
        Ok(()) => {
            db.save();
            println!("Done. Linux programs live in {}/bin and {}/usr/bin.", db::SYSROOT, db::SYSROOT);
            0
        }
        Err(e) => {
            db.save();
            eprintln!("pantry: {}", e);
            1
        }
    }
}

fn cmd_remove(names: &[String]) -> i32 {
    if names.is_empty() {
        eprintln!("usage: pantry remove <package>...");
        return 2;
    }
    if !require_root() {
        return 1;
    }
    let mut db = Db::load();
    match ops::remove(&mut db, names) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("pantry: {}", e);
            1
        }
    }
}

fn cmd_upgrade() -> i32 {
    if !require_root() {
        return 1;
    }
    let Some(cat) = catalog() else {
        return 1;
    };
    let mut db = Db::load();
    let pending = ops::upgrades(&cat, &db);
    if pending.is_empty() {
        println!("Everything is up to date.");
        return 0;
    }
    let names: Vec<String> = pending.iter().map(|(n, _, _)| n.clone()).collect();
    let replace: BTreeSet<String> = names.iter().cloned().collect();
    match ops::plan(&cat, &db, &names, replace).and_then(|order| ops::run_plan(&cat, &mut db, &order)) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("pantry: {}", e);
            1
        }
    }
}

fn cmd_repair() -> i32 {
    if !require_root() {
        return 1;
    }
    let Some(cat) = catalog() else {
        return 1;
    };
    let mut db = Db::load();
    let names: Vec<String> = db.packages.iter().filter(|p| !p.is_system()).map(|p| p.name.clone()).collect();
    if names.is_empty() {
        println!("No Linux packages installed.");
        return 0;
    }
    let replace: BTreeSet<String> = names.iter().cloned().collect();
    match ops::plan(&cat, &db, &names, replace).and_then(|order| ops::run_plan(&cat, &mut db, &order)) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("pantry: {}", e);
            1
        }
    }
}

fn cmd_triggers() -> i32 {
    if !require_root() {
        return 1;
    }
    let db = Db::load();
    let owned: BTreeSet<String> = db.packages.iter().flat_map(|p| p.files.iter()).map(|f| scripts::changed_dir(f)).collect();
    let mut ran = 0;
    for pkg in db.packages.iter().filter(|p| !p.triggers.is_empty() && scripts::has(&p.name, ".trigger")) {
        let mut dirs: Vec<&str> = Vec::new();
        for pattern in &pkg.triggers {
            if pattern.contains('*') || pattern.contains('?') {
                dirs.extend(owned.iter().filter(|dir| scripts::glob_matches(pattern, dir)).map(|dir| dir.as_str()));
            } else if sys::stat(&alloc::format!("{}{}", db::SYSROOT, pattern)).is_ok() {
                dirs.push(pattern.as_str());
            }
        }
        dirs.sort_unstable();
        dirs.dedup();
        if dirs.is_empty() {
            continue;
        }
        println!("Running the {} trigger", pkg.name);
        scripts::run(&pkg.name, ".trigger", &dirs);
        ran += 1;
    }
    if ran == 0 {
        println!("No triggers to run.");
    } else {
        println!("Done, {} trigger(s).", ran);
    }
    0
}

fn cmd_list(args: &[String]) -> i32 {
    let db = Db::load();
    if args.iter().any(|a| a == "--available" || a == "-a") {
        let Some(cat) = catalog() else {
            return 1;
        };
        for name in cat.by_name.keys() {
            if let Some(i) = ops::newest(&cat, name) {
                let mark = if db.find(name).is_some() { " [installed]" } else { "" };
                println!("{} {}{}", name, cat.pkgs[i].version, mark);
            }
        }
        return 0;
    }
    if db.packages.iter().all(|p| p.is_system()) {
        println!("No Linux packages installed yet. Try 'pantry search' and 'pantry install'.");
        return 0;
    }
    let mut pkgs: Vec<&db::Installed> = db.packages.iter().filter(|p| !p.is_system()).collect();
    pkgs.sort_by(|a, b| a.name.cmp(&b.name));
    for p in pkgs {
        let mark = if db.world.contains(&p.name) { "" } else { " (dependency)" };
        println!("{} {}{}", p.name, p.version, mark);
    }
    0
}

fn cmd_files(name: &str) -> i32 {
    let db = Db::load();
    match db.find(name) {
        Some(p) => {
            for f in &p.files {
                println!("{}/{}", db::SYSROOT, f);
            }
            0
        }
        None => {
            eprintln!("pantry: {} is not installed", name);
            1
        }
    }
}

fn cmd_owner(path: &str) -> i32 {
    let db = Db::load();
    match ops::owner(&db, path) {
        Some(name) => {
            println!("{} belongs to {}", path, name);
            0
        }
        None => {
            println!("{} does not belong to any package", path);
            1
        }
    }
}

fn cmd_repos() -> i32 {
    match repo::load_repos() {
        Ok(repos) => {
            for r in repos {
                let kind = match r.kind {
                    index::RepoKind::Alpine => "alpine",
                    index::RepoKind::Hamix => "hamix (not supported yet)",
                };
                println!("{:<12} {:<26} {}", r.name, kind, r.url);
            }
            0
        }
        Err(e) => {
            eprintln!("pantry: {}", e);
            1
        }
    }
}

fn main() -> i32 {
    let args: Vec<String> = env::args().iter().skip(1).cloned().collect();
    let Some(command) = args.first() else {
        return usage();
    };
    let rest = &args[1..];
    let one = |f: fn(&str) -> i32| match rest.first() {
        Some(name) => f(name),
        None => {
            eprintln!("usage: pantry {} <argument>", command);
            2
        }
    };
    match command.as_str() {
        "update" => cmd_update(),
        "search" => cmd_search(rest),
        "info" | "show" => one(cmd_info),
        "install" | "add" => cmd_install(rest),
        "remove" | "del" | "uninstall" => cmd_remove(rest),
        "upgrade" => cmd_upgrade(),
        "repair" => cmd_repair(),
        "triggers" | "retrigger" => cmd_triggers(),
        "list" => cmd_list(rest),
        "files" => one(cmd_files),
        "owner" => one(cmd_owner),
        "repos" => cmd_repos(),
        "version" | "--version" => {
            println!("pantry {}", VERSION);
            0
        }
        "help" | "--help" | "-h" => {
            usage();
            0
        }
        other => {
            eprintln!("pantry: unknown command '{}'", other);
            usage()
        }
    }
}

entry!(main);
