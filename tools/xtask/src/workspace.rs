use std::fs;

use crate::util::*;

const ALWAYS_EXCLUDED: [&str; 3] = ["vendor/smoltcp", "tools/xtask", "modules/e1000e"];

pub fn recipe_dirs() -> Vec<std::path::PathBuf> {
    let mut dirs = Vec::new();
    for group in subdirs(&root().join("ports")) {
        for dir in subdirs(&group) {
            if dir.join("HXBUILD").exists() {
                dirs.push(dir);
            }
        }
    }
    dirs
}

pub fn sync() -> Result {
    let manifest = root().join("Cargo.toml");
    let text = fs::read_to_string(&manifest).map_err(|e| format!("Cargo.toml: {}", e))?;
    let mut excluded: Vec<String> = ALWAYS_EXCLUDED.iter().map(|s| format!("\"{}\"", s)).collect();
    for group in subdirs(&root().join("userland")) {
        for dir in subdirs(&group) {
            if !dir.join("Cargo.toml").exists() {
                let rel = dir.strip_prefix(root()).unwrap_or(&dir).display().to_string();
                excluded.push(format!("\"{}\"", rel));
            }
        }
    }
    let line = format!("exclude = [{}]", excluded.join(", "));
    let mut changed = false;
    let lines: Vec<String> = text
        .lines()
        .map(|l| {
            if l.starts_with("exclude = [") && l != line {
                changed = true;
                line.clone()
            } else {
                String::from(l)
            }
        })
        .collect();
    if changed {
        fs::write(&manifest, lines.join("\n") + "\n").map_err(|e| format!("Cargo.toml: {}", e))?;
    }
    Ok(())
}
