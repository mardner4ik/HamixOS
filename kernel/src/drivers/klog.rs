use alloc::string::String;
use alloc::collections::VecDeque;
use spin::Mutex;

const KLOG_CAP: usize = 1024;

struct KLog {
    lines: VecDeque<String>,
}

impl KLog {
    const fn new() -> Self {
        Self { lines: VecDeque::new() }
    }

    fn push(&mut self, msg: String) {
        if self.lines.len() >= KLOG_CAP {
            self.lines.pop_front();
        }
        self.lines.push_back(msg);
    }
}

static KLOG: Mutex<KLog> = Mutex::new(KLog::new());

/// Records a boot/runtime message in the in-memory ring buffer (visible via
/// the `dmesg` shell command) and mirrors it to the serial console. Takes
/// any `&str`, not just `&'static str`, so callers can log real detected
/// hardware (CPU brand string, GPU chipset, etc) the way Linux's dmesg
/// does, not just fixed boot-stage markers.
pub fn log(msg: &str) {
    KLOG.lock().push(String::from(msg));
    crate::serial_println!("{}", msg);
}

pub fn record(msg: &str) {
    KLOG.lock().push(String::from(msg));
}

pub fn for_each<F: FnMut(&str)>(mut f: F) {
    let klog = KLOG.lock();
    for line in klog.lines.iter() {
        f(line.as_str());
    }
}

#[allow(dead_code)]
pub fn count() -> usize {
    KLOG.lock().lines.len()
}

pub fn tail_without_waiting<F: FnMut(&str)>(lines: usize, mut f: F) {
    let Some(klog) = KLOG.try_lock() else {
        return;
    };
    let start = klog.lines.len().saturating_sub(lines);
    for line in klog.lines.iter().skip(start) {
        f(line.as_str());
    }
}
