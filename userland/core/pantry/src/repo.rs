use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use hamix_std::{fs, println, sys};

use crate::archive;
use crate::crypto::{self, RsaKey};
use crate::db::{self, CACHE_DIR, KEYS_DIR, REPOS_PATH};
use crate::http;
use crate::index::{self, Pkg, Repo, RepoKind};

pub struct Catalog {
    pub repos: Vec<Repo>,
    pub pkgs: Vec<Pkg>,
    pub by_name: BTreeMap<String, Vec<usize>>,
    pub by_provide: BTreeMap<String, Vec<usize>>,
}

pub fn load_repos() -> Result<Vec<Repo>, String> {
    let text = fs::read_to_string(REPOS_PATH).ok_or_else(|| format!("{} is missing", REPOS_PATH))?;
    let (repos, problems) = index::parse_repos(&text);
    for p in problems {
        println!("pantry: {}: {}", REPOS_PATH, p);
    }
    if repos.is_empty() {
        return Err(format!("no repositories configured in {}", REPOS_PATH));
    }
    Ok(repos)
}

pub fn load_keys() -> Vec<(String, RsaKey)> {
    let mut keys = Vec::new();
    for entry in sys::read_dir(KEYS_DIR).unwrap_or_default() {
        if entry.is_dir || !entry.name.ends_with(".pub") {
            continue;
        }
        let path = format!("{}/{}", KEYS_DIR, entry.name);
        match fs::read_to_string(&path).and_then(|t| RsaKey::from_pem(&t)) {
            Some(key) => keys.push((entry.name.clone(), key)),
            None => println!("pantry: ignoring unreadable key {}", path),
        }
    }
    keys
}

pub fn verify_signed(bytes: &[u8], keys: &[(String, RsaKey)]) -> Result<Vec<u8>, String> {
    let members = archive::gzip_members(bytes, 2).map_err(|e| e.to_string())?;
    if members.len() < 2 {
        return Err(String::from("archive is not signed"));
    }
    let signature_tar = archive::tar_entries(&members[0].data).map_err(|e| e.to_string())?;
    for entry in signature_tar {
        let (algorithm, key_name) = if let Some(k) = entry.path.strip_prefix(".SIGN.RSA256.") {
            (256, k)
        } else if let Some(k) = entry.path.strip_prefix(".SIGN.RSA.") {
            (1, k)
        } else {
            continue;
        };
        let Some((_, key)) = keys.iter().find(|(name, _)| name == key_name) else {
            continue;
        };
        let signed = members[1].raw;
        let ok = if algorithm == 256 { key.verify_sha256(&crypto::sha256(signed), entry.data) } else { key.verify_sha1(&crypto::sha1(signed), entry.data) };
        if ok {
            let members = archive::gzip_members(bytes, 2).map_err(|e| e.to_string())?;
            return Ok(members.into_iter().nth(1).map(|m| m.data).unwrap_or_default());
        }
        return Err(format!("bad signature from {}", key_name));
    }
    Err(String::from("no signature from a trusted key"))
}

fn cache_path(repo: &Repo) -> String {
    format!("{}/{}/APKINDEX", CACHE_DIR, repo.name)
}

pub fn update(repos: &[Repo], keys: &[(String, RsaKey)]) -> bool {
    let mut ok = true;
    for repo in repos {
        if repo.kind == RepoKind::Hamix {
            println!("  {}: native HamixOS repositories are not supported yet, skipped", repo.name);
            continue;
        }
        let url = repo.index_url();
        println!("  fetching {}", url);
        let result = http::get(&url, &repo.name, false).and_then(|bytes| verify_signed(&bytes, keys)).and_then(|tar| {
            let entries = archive::tar_entries(&tar).map_err(|e| e.to_string())?;
            let text = entries.iter().find(|e| e.path == "APKINDEX").map(|e| e.data.to_vec()).ok_or("index archive has no APKINDEX")?;
            let description = entries.iter().find(|e| e.path == "DESCRIPTION").map(|e| String::from_utf8_lossy(e.data).trim().to_string()).unwrap_or_default();
            Ok((text, description))
        });
        match result {
            Ok((text, description)) => {
                let count = index::parse_index(&String::from_utf8_lossy(&text), 0).len();
                if db::write_file(&cache_path(repo), &text) {
                    println!("  {}: {} ({} packages, signature ok)", repo.name, description, count);
                } else {
                    println!("  {}: cannot write {}", repo.name, cache_path(repo));
                    ok = false;
                }
            }
            Err(e) => {
                println!("  {}: {}", repo.name, e);
                ok = false;
            }
        }
    }
    sys::sync();
    ok
}

pub fn load_catalog(repos: Vec<Repo>) -> Result<Catalog, String> {
    let mut pkgs = Vec::new();
    let mut missing = Vec::new();
    for (i, repo) in repos.iter().enumerate() {
        if repo.kind != RepoKind::Alpine {
            continue;
        }
        match fs::read_to_string(&cache_path(repo)) {
            Some(text) => pkgs.extend(index::parse_index(&text, i)),
            None => missing.push(repo.name.clone()),
        }
    }
    if pkgs.is_empty() {
        return Err(String::from("no package lists yet, run 'pantry update' first"));
    }
    if !missing.is_empty() {
        println!("pantry: no package list for {} (run 'pantry update')", missing.join(", "));
    }
    let mut by_name: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    let mut by_provide: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, pkg) in pkgs.iter().enumerate() {
        by_name.entry(pkg.name.clone()).or_default().push(i);
        for token in &pkg.provides {
            by_provide.entry(index::provided_name(token).0.to_string()).or_default().push(i);
        }
    }
    Ok(Catalog { repos, pkgs, by_name, by_provide })
}
