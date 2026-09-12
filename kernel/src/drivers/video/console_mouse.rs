use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use alloc::string::String;
use spin::Mutex;

use crate::drivers::input::mouse::{MouseEvent, BUTTON_LEFT, BUTTON_MIDDLE, BUTTON_RIGHT};
use crate::drivers::video::text_mode::{
    self, COLS, POINTER_CELL, ROWS, SELECTION_END, SELECTION_START,
};

static ENABLED: AtomicBool = AtomicBool::new(false);
static DRAGGING: AtomicBool = AtomicBool::new(false);
static ANCHOR: AtomicUsize = AtomicUsize::new(usize::MAX);
static CLIPBOARD: Mutex<String> = Mutex::new(String::new());

pub fn enable() {
    ENABLED.store(true, Ordering::SeqCst);
}

pub fn disable() {
    ENABLED.store(false, Ordering::SeqCst);
    clear_overlay();
}

pub fn clear_overlay() {
    let pointer = POINTER_CELL.swap(usize::MAX, Ordering::SeqCst);
    let start = SELECTION_START.swap(usize::MAX, Ordering::SeqCst);
    let end = SELECTION_END.swap(usize::MAX, Ordering::SeqCst);
    DRAGGING.store(false, Ordering::SeqCst);
    if pointer != usize::MAX {
        redraw_index(pointer);
    }
    if start != usize::MAX {
        redraw_range(start.min(end), start.max(end));
    }
}

pub fn clipboard() -> String {
    CLIPBOARD.lock().clone()
}

fn redraw_index(index: usize) {
    if index < COLS * ROWS {
        text_mode::render_cell(index % COLS, index / COLS);
    }
}

fn redraw_range(lo: usize, hi: usize) {
    let hi = hi.min(COLS * ROWS - 1);
    for index in lo..=hi {
        redraw_index(index);
    }
}

fn selection_bounds() -> Option<(usize, usize)> {
    let start = SELECTION_START.load(Ordering::Relaxed);
    if start == usize::MAX {
        return None;
    }
    let end = SELECTION_END.load(Ordering::Relaxed);
    Some((start.min(end), start.max(end)))
}

fn copy_selection() {
    let Some((lo, hi)) = selection_bounds() else {
        return;
    };
    let mut text = String::new();
    let mut index = lo;
    while index <= hi {
        let row_end = ((index / COLS) * COLS + COLS - 1).min(hi);
        let mut line = String::new();
        for cell in index..=row_end {
            line.push(text_mode::cell_char(cell));
        }
        while line.ends_with(' ') {
            line.pop();
        }
        text.push_str(&line);
        if row_end < hi {
            text.push('\n');
        }
        index = row_end + 1;
    }
    *CLIPBOARD.lock() = text;
}

fn paste() {
    let text = CLIPBOARD.lock().clone();
    if !text.is_empty() {
        crate::drivers::input::keyboard::push_text(&text);
    }
}

pub fn on_event(event: MouseEvent) {
    if !ENABLED.load(Ordering::Relaxed) || text_mode::graphics_owned() {
        return;
    }
    let Some((col, row)) = text_mode::cell_at_pixel(event.x, event.y) else {
        return;
    };
    let index = row * COLS + col;

    let previous_selection = selection_bounds();

    if event.pressed & BUTTON_LEFT != 0 {
        if let Some((lo, hi)) = previous_selection {
            SELECTION_START.store(usize::MAX, Ordering::SeqCst);
            SELECTION_END.store(usize::MAX, Ordering::SeqCst);
            redraw_range(lo, hi);
        }
        ANCHOR.store(index, Ordering::SeqCst);
        DRAGGING.store(true, Ordering::SeqCst);
        SELECTION_START.store(index, Ordering::SeqCst);
        SELECTION_END.store(index, Ordering::SeqCst);
    } else if DRAGGING.load(Ordering::Relaxed) && event.buttons & BUTTON_LEFT != 0 {
        SELECTION_END.store(index, Ordering::SeqCst);
    }

    if event.released & BUTTON_LEFT != 0 && DRAGGING.swap(false, Ordering::SeqCst) {
        copy_selection();
    }

    if event.pressed & BUTTON_RIGHT != 0 {
        if let Some((lo, hi)) = selection_bounds() {
            SELECTION_START.store(usize::MAX, Ordering::SeqCst);
            SELECTION_END.store(usize::MAX, Ordering::SeqCst);
            redraw_range(lo, hi);
        }
    }

    if event.pressed & BUTTON_MIDDLE != 0 {
        paste();
    }

    let previous_pointer = POINTER_CELL.swap(index, Ordering::SeqCst);

    if let Some((lo, hi)) = selection_bounds() {
        let (old_lo, old_hi) = previous_selection.unwrap_or((lo, hi));
        redraw_range(lo.min(old_lo), hi.max(old_hi));
    }

    if previous_pointer != index {
        if previous_pointer != usize::MAX {
            redraw_index(previous_pointer);
        }
        redraw_index(index);
    }
}
