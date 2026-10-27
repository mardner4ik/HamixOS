use alloc::vec::Vec;
use spin::Mutex;

use super::frame;

const PAGE_SIZE: usize = 4096;
const MAX_PAGES: usize = 1024;

struct Entry {
    node: usize,
    offset: u64,
    frame: usize,
    age: u64,
    generation: u64,
}

struct Cache {
    entries: Vec<Entry>,
    generations: Vec<(usize, u64)>,
    age: u64,
}

impl Cache {
    const fn new() -> Self {
        Self { entries: Vec::new(), generations: Vec::new(), age: 0 }
    }

    fn generation(&mut self, node: usize) -> u64 {
        if let Some((_, generation)) = self.generations.iter().find(|(id, _)| *id == node) {
            return *generation;
        }
        self.generations.push((node, 1));
        1
    }

    fn invalidate(&mut self, node: usize) {
        if let Some((_, generation)) = self.generations.iter_mut().find(|(id, _)| *id == node) {
            *generation = generation.wrapping_add(1).max(1);
        } else {
            self.generations.push((node, 2));
        }
    }

    fn tick(&mut self) -> u64 {
        self.age = self.age.wrapping_add(1).max(1);
        self.age
    }

    fn find(&mut self, node: usize, offset: u64, generation: u64) -> Option<usize> {
        let age = self.tick();
        let entry = self.entries.iter_mut().find(|entry| entry.node == node && entry.offset == offset && entry.generation == generation)?;
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
        if let Some((index, _)) = cache.entries.iter().enumerate().filter(|(_, entry)| !frame::is_shared(entry.frame)).min_by_key(|(_, entry)| entry.age) {
            let stale = cache.entries.swap_remove(index);
            frame::free_frame(stale.frame);
        } else {
            return Some(phys);
        }
    }
    let age = cache.tick();
    cache.entries.push(Entry { node, offset, frame: phys, age, generation });
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
    let mut index = 0;
    while index < cache.entries.len() {
        if cache.entries[index].node == node {
            let stale = cache.entries.swap_remove(index);
            frame::free_frame(stale.frame);
        } else {
            index += 1;
        }
    }
}
