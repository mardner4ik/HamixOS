use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use spin::Mutex;

use super::frame;

const PAGE_SIZE: usize = 4096;
const MAX_PAGES: usize = 1024;

struct Entry {
    frame: usize,
    age: u64,
    generation: u64,
}

struct Cache {
    entries: BTreeMap<(usize, u64), Entry>,
    generations: Vec<u64>,
    age: u64,
}

impl Cache {
    const fn new() -> Self {
        Self { entries: BTreeMap::new(), generations: Vec::new(), age: 0 }
    }

    fn generation(&mut self, node: usize) -> u64 {
        if self.generations.len() <= node {
            self.generations.resize(node + 1, 1);
        }
        self.generations[node]
    }

    fn invalidate(&mut self, node: usize) {
        if self.generations.len() <= node {
            self.generations.resize(node + 1, 1);
        }
        self.generations[node] = self.generations[node].wrapping_add(1).max(1);
    }

    fn tick(&mut self) -> u64 {
        self.age = self.age.wrapping_add(1).max(1);
        self.age
    }

    fn find(&mut self, node: usize, offset: u64, generation: u64) -> Option<usize> {
        let age = self.tick();
        let entry = self.entries.get_mut(&(node, offset))?;
        if entry.generation != generation {
            return None;
        }
        entry.age = age;
        Some(entry.frame)
    }
}

static CACHE: Mutex<Cache> = Mutex::new(Cache::new());

pub fn get_or_load<F>(node: usize, offset: u64, load: F) -> Option<usize>
where
    F: FnOnce(&mut [u8]) -> bool,
{
    let (generation, cached) = {
        let mut cache = CACHE.lock();
        let generation = cache.generation(node);
        let cached = cache.find(node, offset, generation).map(|phys| (phys, frame::share(phys)));
        (generation, cached)
    };
    if let Some((phys, true)) = cached {
        return Some(phys);
    }
    if let Some((phys, false)) = cached {
        let copy = frame::alloc_frame()?;
        unsafe { core::ptr::copy_nonoverlapping(phys as *const u8, copy as *mut u8, PAGE_SIZE) };
        return Some(copy);
    }

    let phys = frame::alloc_zeroed_frame()?;
    let loaded = load(unsafe { core::slice::from_raw_parts_mut(phys as *mut u8, PAGE_SIZE) });
    if !loaded {
        frame::free_frame(phys);
        return None;
    }

    let mut cache = CACHE.lock();
    if cache.generation(node) != generation {
        return Some(phys);
    }
    if let Some(existing) = cache.find(node, offset, generation) {
        frame::free_frame(phys);
        return if frame::share(existing) {
            Some(existing)
        } else {
            let copy = frame::alloc_frame()?;
            unsafe { core::ptr::copy_nonoverlapping(existing as *const u8, copy as *mut u8, PAGE_SIZE) };
            Some(copy)
        };
    }

    if cache.entries.len() >= MAX_PAGES {
        let victim = cache.entries.iter().filter(|(_, entry)| !frame::is_shared(entry.frame)).min_by_key(|(_, entry)| entry.age).map(|(key, _)| *key);
        if let Some(key) = victim {
            if let Some(stale) = cache.entries.remove(&key) {
                frame::free_frame(stale.frame);
            }
        } else {
            return Some(phys);
        }
    }
    let age = cache.tick();
    cache.entries.insert((node, offset), Entry { frame: phys, age, generation });
    if frame::share(phys) {
        Some(phys)
    } else {
        let copy = frame::alloc_frame()?;
        unsafe { core::ptr::copy_nonoverlapping(phys as *const u8, copy as *mut u8, PAGE_SIZE) };
        Some(copy)
    }
}

pub fn invalidate(node: usize) {
    let mut cache = CACHE.lock();
    cache.invalidate(node);
    let keys: Vec<(usize, u64)> = cache.entries.range((node, 0)..=(node, u64::MAX)).map(|(key, _)| *key).collect();
    for key in keys {
        if let Some(stale) = cache.entries.remove(&key) {
            frame::free_frame(stale.frame);
        }
    }
}
