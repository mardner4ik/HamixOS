use core::sync::atomic::{AtomicU64, Ordering};

static STATE: AtomicU64 = AtomicU64::new(0);

fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

pub fn next_u64() -> u64 {
    let cycles = crate::arch::platform::cycles();
    let step = cycles | 1;
    let state = STATE.fetch_add(step.wrapping_mul(0x9E37_79B9_7F4A_7C15), Ordering::Relaxed);
    let soft = mix(state ^ crate::arch::platform::cycles().rotate_left(13) ^ crate::task::ticks().rotate_left(32));
    match crate::arch::platform::hw_random() {
        Some(hw) => hw ^ soft,
        None => soft,
    }
}

pub fn fill(buf: &mut [u8]) {
    for chunk in buf.chunks_mut(8) {
        let bytes = next_u64().to_le_bytes();
        chunk.copy_from_slice(&bytes[..chunk.len()]);
    }
}
