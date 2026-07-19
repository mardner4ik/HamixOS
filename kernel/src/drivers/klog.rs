use spin::Mutex;

const KLOG_CAP: usize = 64;

struct KLog {
    lines: [Option<&'static str>; KLOG_CAP],
    len: usize,
}

impl KLog {
    const fn new() -> Self {
        Self {
            lines: [None; KLOG_CAP],
            len: 0,
        }
    }

    fn push(&mut self, msg: &'static str) {
        if self.len < KLOG_CAP {
            self.lines[self.len] = Some(msg);
            self.len += 1;
        } else {
            for i in 1..KLOG_CAP {
                self.lines[i - 1] = self.lines[i];
            }
            self.lines[KLOG_CAP - 1] = Some(msg);
        }
    }
}

static KLOG: Mutex<KLog> = Mutex::new(KLog::new());

pub fn log(msg: &'static str) {
    KLOG.lock().push(msg);
    crate::serial_println!("{}", msg);
}

pub fn for_each<F: FnMut(&str)>(mut f: F) {
    let klog = KLOG.lock();
    for i in 0..klog.len {
        if let Some(line) = klog.lines[i] {
            f(line);
        }
    }
}

#[allow(dead_code)]
pub fn count() -> usize {
    KLOG.lock().len
}
