use super::*;

pub(in crate::syscall) const SYS_AUDIO_OPEN: u64 = 9170;
pub(in crate::syscall) const SYS_AUDIO_WRITE: u64 = 9171;
pub(in crate::syscall) const SYS_AUDIO_STATUS: u64 = 9172;
pub(in crate::syscall) const SYS_AUDIO_CONTROL: u64 = 9173;
pub(in crate::syscall) const SYS_AUDIO_CLOSE: u64 = 9174;
pub(in crate::syscall) const SYS_AUDIO_VOLUME: u64 = 9175;
pub(in crate::syscall) const SYS_AUDIO_INFO: u64 = 9176;

pub(super) fn dispatch(number: u64, a1: u64, a2: u64, a3: u64, a4: u64, _a5: u64, _a6: u64) -> Option<i64> {
    Some(match number {
        SYS_AUDIO_OPEN => audio::open(task::current_pid(), a1 as u32, a2 as u32, a3 as u32),
        SYS_AUDIO_WRITE => match user_slice(a2, a3.min(4 << 20)) {
            Ok(data) => audio::write(task::current_pid(), a1 as u32, data, a4 & 1 == 0),
            Err(e) => e,
        },
        SYS_AUDIO_STATUS => match audio::status(task::current_pid(), a1 as u32) {
            Some(values) => {
                let mut bytes = [0u8; 64];
                for (i, v) in values.iter().enumerate() {
                    bytes[i * 8..i * 8 + 8].copy_from_slice(&v.to_le_bytes());
                }
                copy_out(a2, a3.min(64), &bytes).min(0)
            }
            None => EBADF,
        },
        SYS_AUDIO_CONTROL => audio::control(task::current_pid(), a1 as u32, a2, a3),
        SYS_AUDIO_CLOSE => audio::close(task::current_pid(), a1 as u32, a2 != 0),
        SYS_AUDIO_VOLUME => {
            match a1 {
                1 => audio::set_volume(a2 as u32),
                2 => audio::set_muted(a2 != 0),
                3 => audio::step_volume(a2 as i64 as i32),
                4 => audio::toggle_mute(),
                _ => {}
            }
            if audio::present() { audio::volume_word() } else { ENODEV }
        }
        SYS_AUDIO_INFO => copy_out(a1, a2, audio::info_text().as_bytes()),
        _ => return None,
    })
}
