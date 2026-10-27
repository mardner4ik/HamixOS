use alloc::string::String;
use alloc::vec::Vec;

static mut ARGS: Vec<String> = Vec::new();
static mut VARS: Vec<(String, String)> = Vec::new();

unsafe fn c_string(ptr: *const u8) -> String {
    unsafe {
        let mut len = 0usize;
        while *ptr.add(len) != 0 && len < 65536 {
            len += 1;
        }
        String::from_utf8_lossy(core::slice::from_raw_parts(ptr, len)).into_owned()
    }
}

pub(crate) unsafe fn init(stack: *const u64) {
    unsafe {
        let argc = *stack as usize;
        let mut args = Vec::with_capacity(argc);
        for i in 0..argc.min(256) {
            let ptr = *stack.add(1 + i) as *const u8;
            if ptr.is_null() {
                break;
            }
            args.push(c_string(ptr));
        }
        let mut vars = Vec::new();
        let mut cursor = stack.add(2 + argc);
        while vars.len() < 4096 {
            let ptr = *cursor as *const u8;
            if ptr.is_null() {
                break;
            }
            if let Some((name, value)) = c_string(ptr).split_once('=') {
                if !name.is_empty() {
                    vars.push((String::from(name), String::from(value)));
                }
            }
            cursor = cursor.add(1);
        }
        *(&raw mut ARGS) = args;
        *(&raw mut VARS) = vars;
    }
}

pub fn vars() -> &'static [(String, String)] {
    unsafe { (*(&raw const VARS)).as_slice() }
}

pub fn var(name: &str) -> Option<&'static str> {
    vars().iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
}

pub fn args() -> &'static [String] {
    unsafe { (*(&raw const ARGS)).as_slice() }
}

pub fn program() -> &'static str {
    args().first().map(|s| s.as_str()).unwrap_or("")
}
