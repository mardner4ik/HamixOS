use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use super::*;
use crate::drivers::audio;
use crate::drivers::block;
use crate::fs;
use crate::task::{self, ipc, pipe, registry, Pid};

pub(super) const SYS_HAMIX_FBMAP: u64 = 9001;
pub(super) const SYS_HAMIX_READKEY: u64 = 9002;
pub(super) const SYS_HAMIX_TRUNCATE: u64 = 9003;
pub(super) const SYS_HAMIX_MOUSE: u64 = 9004;
pub(super) const SYS_HAMIX_POLLKEY: u64 = 9005;
pub(super) const SYS_HAMIX_RELEASE_FB: u64 = 9006;
pub(super) const SYS_HAMIX_SPAWN: u64 = 9010;
pub(super) const SYS_HAMIX_WAITPID: u64 = 9012;
pub(super) const SYS_HAMIX_SLEEP: u64 = 9013;
pub(super) const SYS_HAMIX_WAIT_EVENT: u64 = 9014;
pub(super) const SYS_HAMIX_KILL: u64 = 9015;
pub(super) const SYS_HAMIX_PROC_LIST: u64 = 9016;
pub(super) const SYS_HAMIX_SYSINFO: u64 = 9017;
pub(super) const SYS_HAMIX_PROC_ALIVE: u64 = 9018;
pub(super) const SYS_HAMIX_MSG_SEND: u64 = 9020;
pub(super) const SYS_HAMIX_MSG_RECV: u64 = 9021;
pub(super) const SYS_HAMIX_SERVICE_REGISTER: u64 = 9022;
pub(super) const SYS_HAMIX_SERVICE_LOOKUP: u64 = 9023;
pub(super) const SYS_HAMIX_MAILBOX_FD: u64 = 9024;
pub(super) const SYS_HAMIX_SHM_CREATE: u64 = 9030;
pub(super) const SYS_HAMIX_SHM_MAP: u64 = 9031;
pub(super) const SYS_HAMIX_SHM_RELEASE: u64 = 9032;
pub(super) const SYS_HAMIX_CMD_REGISTER: u64 = 9040;
pub(super) const SYS_HAMIX_CMD_UNREGISTER: u64 = 9041;
pub(super) const SYS_HAMIX_CMD_LIST: u64 = 9042;
pub(super) const SYS_HAMIX_CMD_RUN: u64 = 9043;
pub(super) const SYS_HAMIX_SYNC: u64 = 9050;
pub(super) const SYS_HAMIX_READDIR: u64 = 9051;
pub(super) const SYS_HAMIX_STAT: u64 = 9052;
pub(super) const SYS_HAMIX_REALTIME: u64 = 9060;
pub(super) const SYS_HAMIX_DISK_LIST: u64 = 9070;
pub(super) const SYS_HAMIX_DISK_IO: u64 = 9071;
pub(super) const SYS_HAMIX_DISK_RESCAN: u64 = 9072;
pub(super) const SYS_HAMIX_MKFS: u64 = 9073;
pub(super) const SYS_HAMIX_MOUNT: u64 = 9074;
pub(super) const SYS_HAMIX_UMOUNT: u64 = 9075;
pub(super) const SYS_HAMIX_PIPE: u64 = 9090;
pub(super) const SYS_HAMIX_ISATTY: u64 = 9093;
pub(super) const SYS_HAMIX_TERMSIZE: u64 = 9094;
pub(super) const SYS_HAMIX_SET_TERMINAL: u64 = 9095;
pub(super) const SYS_HAMIX_TERMGEN: u64 = 9097;
pub(super) const SYS_HAMIX_SET_FOREGROUND: u64 = 9096;
pub(super) const SYS_HAMIX_AUTH: u64 = 9100;
pub(super) const SYS_HAMIX_USERS_RELOAD: u64 = 9101;
pub(super) const SYS_HAMIX_POWER: u64 = 9110;
pub(super) const SYS_HAMIX_SET_OWN_PASSWORD: u64 = 9102;
pub(super) const SYS_HAMIX_TRACE: u64 = 9103;
pub(super) const SYS_HAMIX_CPU_STATS: u64 = 9120;
pub(super) const SYS_NET_STATUS: u64 = 9130;
pub(super) const SYS_NET_SET_MODE: u64 = 9131;
pub(super) const SYS_WIFI_SCAN: u64 = 9132;
pub(super) const SYS_WIFI_CONNECT: u64 = 9133;
pub(super) const SYS_WIFI_DISCONNECT: u64 = 9134;
pub(super) const SYS_NET_CONFIGURE: u64 = 9135;
pub(super) const SYS_WIFI_KNOWN: u64 = 9136;
pub(super) const SYS_WIFI_FORGET: u64 = 9137;
pub(super) const SYS_WIFI_AUTOJOIN: u64 = 9138;
pub(super) const SYS_SOCKET: u64 = 9140;
pub(super) const SYS_SOCKET_CONNECT: u64 = 9141;
pub(super) const SYS_SOCKET_SEND: u64 = 9142;
pub(super) const SYS_SOCKET_RECV: u64 = 9143;
pub(super) const SYS_SOCKET_CLOSE: u64 = 9144;
pub(super) const SYS_RESOLVE: u64 = 9145;
pub(super) const SYS_PING: u64 = 9146;
pub(super) const SYS_SOCKET_LISTEN: u64 = 9147;
pub(super) const SYS_SOCKET_STATE: u64 = 9148;
pub(super) const SYS_SOCKET_SENDTO: u64 = 9149;
pub(super) const SYS_SOCKET_RECVFROM: u64 = 9150;
pub(super) const SYS_DISPLAY_INFO: u64 = 9160;
pub(super) const SYS_DISPLAY_SET: u64 = 9161;
pub(super) const SYS_DISPLAY_CONFIRM: u64 = 9162;
pub(super) const SYS_DISPLAY_REVERT: u64 = 9163;
pub(super) const SYS_FB_GENERATION: u64 = 9164;
pub(super) const SYS_AUDIO_OPEN: u64 = 9170;
pub(super) const SYS_AUDIO_WRITE: u64 = 9171;
pub(super) const SYS_AUDIO_STATUS: u64 = 9172;
pub(super) const SYS_AUDIO_CONTROL: u64 = 9173;
pub(super) const SYS_AUDIO_CLOSE: u64 = 9174;
pub(super) const SYS_AUDIO_VOLUME: u64 = 9175;
pub(super) const SYS_AUDIO_INFO: u64 = 9176;
pub(super) const SYS_GPU_CAPS: u64 = 9180;
pub(super) const SYS_GPU_FLUSH: u64 = 9181;
pub(super) const SYS_GPU_FILL: u64 = 9182;
pub(super) const SYS_GPU_COPY: u64 = 9183;
pub(super) const SYS_GPU_CURSOR_SET: u64 = 9184;
pub(super) const SYS_GPU_CURSOR_MOVE: u64 = 9185;
pub(super) const SYS_GPU_CURSOR_HIDE: u64 = 9186;
pub(super) const SYS_GPU_FLIP: u64 = 9187;
pub(super) const SYS_GPU_VBLANK: u64 = 9188;
pub(super) const SYS_GPU_BUFFERS: u64 = 9189;
pub(super) const SYS_DISPLAY_EDID: u64 = 9165;
pub(super) const SYS_DISPLAY_OUTPUT: u64 = 9166;
pub(super) const SPAWN_DETACH: u64 = 1;
pub(super) const SPAWN_FOREGROUND: u64 = 2;
pub(super) const SPAWN_ROOT: u64 = 4;
pub(super) const SPAWN_ENV: u64 = 8;
pub(super) const MAX_ENV_ENTRIES: usize = 1024;
pub(super) const EVENT_INPUT: u64 = 1;
pub(super) const EVENT_MESSAGE: u64 = 2;
pub(super) const EVENT_PIPE: u64 = 4;
pub(super) const ROOT_TICKET_TICKS: u64 = task::TICK_HZ * 300;

pub(super) fn dispatch(number: u64, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64, a6: u64) -> Option<i64> {
    Some(match number {
        SYS_HAMIX_MAILBOX_FD => super::linux::epoll::sys_mailbox_fd(),
        SYS_HAMIX_FBMAP => sys_fbmap(a1),
        SYS_HAMIX_RELEASE_FB => {
            task::display::release(task::current_pid());
            0
        }
        SYS_GPU_CAPS => crate::drivers::video::gpu::caps() as i64,
        SYS_GPU_FLUSH => sys_gpu_flush(a1, a2),
        SYS_GPU_FILL => sys_gpu_fill(a1, a2, a3),
        SYS_GPU_COPY => sys_gpu_copy(a1, a2, a3),
        SYS_GPU_CURSOR_SET => sys_gpu_cursor_set(a1, a2, a3),
        SYS_GPU_CURSOR_MOVE => sys_gpu_cursor(a1, false),
        SYS_GPU_CURSOR_HIDE => sys_gpu_cursor(0, true),
        SYS_GPU_FLIP => {
            if !gpu_allowed() {
                return Some(EACCES);
            }
            crate::drivers::video::gpu::page_flip(a1 as u32) as i64
        }
        SYS_GPU_VBLANK => crate::drivers::video::gpu::wait_vblank(a1.clamp(1, 1000) as u32) as i64,
        SYS_GPU_BUFFERS => crate::drivers::video::gpu::buffers() as i64,
        SYS_HAMIX_MOUSE => sys_mouse(a1),
        SYS_HAMIX_POLLKEY => poll_key().unwrap_or(-100),
        SYS_HAMIX_READKEY => read_key_code(),
        SYS_HAMIX_TRUNCATE => sys_truncate(a1),
        SYS_HAMIX_SPAWN => sys_spawn(a1, a2, a3, a4, a5, a6),
        SYS_HAMIX_WAITPID => sys_waitpid(a1, a2),
        SYS_HAMIX_SLEEP => {
            sleep_ms(a1);
            0
        }
        SYS_HAMIX_WAIT_EVENT => sys_wait_event(a1, a2 as i64),
        SYS_HAMIX_KILL => sys_kill(a1),
        SYS_HAMIX_PROC_LIST => sys_proc_list(a1, a2),
        SYS_HAMIX_SYSINFO => sys_sysinfo(a1, a2),
        SYS_HAMIX_PROC_ALIVE => task::exists(a1 as Pid) as i64,
        SYS_HAMIX_MSG_SEND => match user_slice(a2, a3) {
            Ok(data) => ipc::send(task::current_pid(), a1 as Pid, data),
            Err(e) => e,
        },
        SYS_HAMIX_MSG_RECV => sys_msg_recv(a1, a2, a3, a4 as i64),
        SYS_HAMIX_SERVICE_REGISTER => match user_string(a1, a2) {
            Ok(name) => ipc::register_service(&name, task::current_pid()),
            Err(e) => e,
        },
        SYS_HAMIX_SERVICE_LOOKUP => match user_string(a1, a2) {
            Ok(name) => ipc::lookup_service(&name),
            Err(e) => e,
        },
        SYS_HAMIX_SHM_CREATE => ipc::shm_create(task::current_pid(), a1),
        SYS_HAMIX_SHM_MAP => match ipc::shm_map(task::current_pid(), a1 as u32) {
            Ok((addr, len)) => {
                if a2 != 0 {
                    if let Ok(out) = user_slice(a2, 8) {
                        out.copy_from_slice(&len.to_le_bytes());
                    }
                }
                addr as i64
            }
            Err(e) => e,
        },
        SYS_HAMIX_SHM_RELEASE => ipc::shm_release(task::current_pid(), a1 as u32),
        SYS_HAMIX_CMD_REGISTER => sys_cmd_register(a1, a2, a3, a4),
        SYS_HAMIX_CMD_UNREGISTER => match user_string(a1, a2) {
            Ok(name) => match registry::unregister(&name, euid()) {
                Ok(()) => 0,
                Err(_) => EACCES,
            },
            Err(e) => e,
        },
        SYS_HAMIX_CMD_LIST => copy_out(a1, a2, registry::render().as_bytes()),
        SYS_HAMIX_CMD_RUN => sys_cmd_run(a1, a2, a3, a4),
        SYS_HAMIX_SYNC => {
            fs::sync();
            0
        }
        SYS_HAMIX_READDIR => sys_readdir(a1, a2, a3, a4),
        SYS_HAMIX_STAT => sys_stat(a1, a2),
        SYS_HAMIX_REALTIME => crate::drivers::rtc::now() as i64,
        SYS_HAMIX_DISK_LIST => copy_out(a1, a2, disk_listing().as_bytes()),
        SYS_HAMIX_DISK_IO => sys_disk_io(a1, a2, a3, a4, a5),
        SYS_HAMIX_DISK_RESCAN => {
            block::rescan();
            fs::refresh_block_nodes();
            0
        }
        SYS_HAMIX_MKFS => sys_mkfs(a1, a2),
        SYS_HAMIX_MOUNT => sys_mount(a1, a2),
        SYS_HAMIX_UMOUNT => sys_umount(a1),
        SYS_HAMIX_PIPE => sys_pipe(a1, 0),
        SYS_HAMIX_ISATTY => is_tty(a1) as i64,
        SYS_HAMIX_TERMSIZE => match term_size(a1) {
            Some((cols, rows)) => ((cols as i64) << 16) | rows as i64,
            None => ENOTTY,
        },
        SYS_HAMIX_SET_TERMINAL => match target(a1) {
            Target::PipeRead(id) | Target::PipeWrite(id) => {
                if pipe::mark_terminal(id, a2 as u16, a3 as u16, matches!(target(a1), Target::PipeRead(_))) {
                    notify_resize(id);
                }
                0
            }
            Target::PtyMaster(input, output) | Target::Pty(input, output) => {
                pipe::set_terminal(output, a2 as u16, a3 as u16);
                if pipe::set_terminal(input, a2 as u16, a3 as u16) {
                    notify_resize(input);
                }
                0
            }
            _ => ENOTTY,
        },
        SYS_HAMIX_TERMGEN => match target(a1) {
            Target::Console => 0,
            Target::PipeRead(id) | Target::PipeWrite(id) | Target::Pty(id, _) => match pipe::terminal_generation(id) {
                Some(generation) => generation as i64,
                None => ENOTTY,
            },
            _ => ENOTTY,
        },
        SYS_HAMIX_SET_FOREGROUND => sys_set_foreground(a1),
        SYS_HAMIX_AUTH => sys_auth(a1, a2),
        SYS_HAMIX_USERS_RELOAD => {
            crate::users::reload();
            0
        }
        SYS_HAMIX_POWER => sys_power(a1),
        SYS_HAMIX_CPU_STATS => copy_out(a1, a2, cpu_stats_text().as_bytes()),
        SYS_NET_STATUS => copy_out(a1, a2, crate::net::status_text().as_bytes()),
        SYS_NET_SET_MODE => {
            if let Err(e) = require_root_or_console() {
                return Some(e);
            }
            match crate::net::set_mode(a1 as u8) {
                Ok(()) => 0,
                Err(_) => ENODEV,
            }
        }
        SYS_WIFI_SCAN => match crate::net::wifi_scan_text(a3 != 0) {
            Ok(text) => copy_out(a1, a2, text.as_bytes()),
            Err(_) => ENODEV,
        },
        SYS_WIFI_CONNECT => match (user_string(a1, a2), user_string(a3, a4)) {
            (Ok(ssid), Ok(pass)) => match crate::net::wifi_connect(&ssid, &pass) {
                Ok(()) => 0,
                Err("no Wi-Fi adapter") => ENODEV,
                Err(_) => EINVAL,
            },
            (Err(e), _) | (_, Err(e)) => e,
        },
        SYS_WIFI_DISCONNECT => match crate::net::wifi_disconnect() {
            Ok(()) => 0,
            Err(_) => ENODEV,
        },
        SYS_WIFI_KNOWN => copy_out(a1, a2, crate::net::wifi_known_text().as_bytes()),
        SYS_WIFI_FORGET => match user_string(a1, a2) {
            Ok(ssid) => {
                if task::with_current(|t| t.uid) != 0 {
                    EPERM
                } else if crate::net::wifi_forget(&ssid) {
                    0
                } else {
                    ENOENT
                }
            }
            Err(e) => e,
        },
        SYS_WIFI_AUTOJOIN => match user_string(a1, a2) {
            Ok(ssid) => {
                if task::with_current(|t| t.uid) != 0 {
                    EPERM
                } else if crate::net::known::set_automatic(&ssid, a3 != 0) {
                    0
                } else {
                    ENOENT
                }
            }
            Err(e) => e,
        },
        SYS_NET_CONFIGURE => {
            if let Err(e) = require_root() {
                return Some(e);
            }
            match user_string(a3, a4) {
                Ok(spec) => match crate::net::stack::configure(&spec) {
                    Ok(()) => 0,
                    Err(_) => EINVAL,
                },
                Err(e) => e,
            }
        }
        SYS_SOCKET => crate::net::stack::socket(a1, a2 as u16),
        SYS_SOCKET_CONNECT => crate::net::stack::connect(a1, ((a2 >> 16) as u32).to_be_bytes(), a2 as u16, a3),
        SYS_SOCKET_LISTEN => crate::net::stack::listen(a1, a2 as u16),
        SYS_SOCKET_STATE => crate::net::stack::state(a1),
        SYS_SOCKET_SEND => match user_slice(a2, a3.min(1 << 20)) {
            Ok(data) => {
                let copy = data.to_vec();
                crate::net::stack::send(a1, &copy)
            }
            Err(e) => e,
        },
        SYS_SOCKET_RECV => {
            let mut tmp = alloc::vec![0u8; a3.min(1 << 20) as usize];
            let n = crate::net::stack::recv(a1, &mut tmp, a4);
            if n > 0 { copy_out(a2, a3, &tmp[..n as usize]).min(n) } else { n }
        }
        SYS_SOCKET_SENDTO => match user_slice(a3, a4.min(65536)) {
            Ok(data) => {
                let copy = data.to_vec();
                crate::net::stack::send_to(a1, ((a2 >> 16) as u32).to_be_bytes(), a2 as u16, &copy)
            }
            Err(e) => e,
        },
        SYS_SOCKET_RECVFROM => {
            let mut tmp = alloc::vec![0u8; a3.min(65536) as usize];
            match crate::net::stack::recv_from(a1, &mut tmp, a4) {
                Ok((n, ip, port)) => {
                    if a5 != 0 {
                        let mut meta = [0u8; 8];
                        meta[..4].copy_from_slice(&ip);
                        meta[4..6].copy_from_slice(&port.to_le_bytes());
                        copy_out(a5, 8, &meta);
                    }
                    copy_out(a2, a3, &tmp[..n]).min(n as i64)
                }
                Err(e) => e,
            }
        }
        SYS_SOCKET_CLOSE => crate::net::stack::close(a1),
        SYS_RESOLVE => match user_string(a1, a2) {
            Ok(name) => match crate::net::stack::resolve(&name, 5000) {
                Ok(ip) => copy_out(a3, 4, &ip).min(0),
                Err(e) => e,
            },
            Err(e) => e,
        },
        SYS_PING => crate::net::stack::ping((a1 as u32).to_be_bytes(), a2 as u16, a3.clamp(100, 10_000)),
        SYS_DISPLAY_INFO => copy_out(a1, a2, crate::drivers::video::modes::info_text().as_bytes()),
        SYS_DISPLAY_SET => {
            if let Err(e) = require_root_or_console() {
                return Some(e);
            }
            crate::drivers::video::modes::set_mode((a1 >> 32) as u32, a1 as u32, a2 as u32, a3 != 0)
        }
        SYS_DISPLAY_CONFIRM => crate::drivers::video::modes::confirm(),
        SYS_DISPLAY_REVERT => crate::drivers::video::modes::revert(),
        SYS_FB_GENERATION => crate::drivers::video::modes::generation() as i64,
        SYS_DISPLAY_EDID => {
            let raw = crate::drivers::video::gpu::edid(a1 as u32);
            if raw.is_empty() {
                return Some(ENOENT);
            }
            copy_out(a2, a3, &raw)
        }
        SYS_DISPLAY_OUTPUT => {
            if let Err(e) = require_root_or_console() {
                return Some(e);
            }
            crate::drivers::video::gpu::set_output(a1 as u32, (a2 >> 32) as u32, a2 as u32) as i64
        }
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
        SYS_HAMIX_TRACE => sys_trace(a1, a2),
        SYS_HAMIX_SET_OWN_PASSWORD => match (user_cstr(a1), user_cstr(a2)) {
            (Ok(old), Ok(new)) => {
                let ruid = task::with_current(|t| t.ruid);
                crate::users::reload();
                let name = crate::users::name_of(ruid);
                match name.map(|n| crate::users::set_own_password(&n, &old, &new)) {
                    Some(Ok(())) => {
                        fs::request_sync();
                        0
                    }
                    _ => {
                        sleep_ms(800);
                        EACCES
                    }
                }
            }
            (Err(e), _) | (_, Err(e)) => e,
        },
        _ => return None,
    })
}

pub(super) fn sys_truncate(path_ptr: u64) -> i64 {
    let path = match path_arg(path_ptr) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let uid = euid();
    let mut guard = fs::VFS.lock();
    let Some(vfs) = guard.as_mut() else {
        return ENODEV;
    };
    let root = vfs.root_id();
    match vfs.resolve(root, &path) {
        Some(id) => match vfs.truncate_node(id, uid) {
            Ok(()) => 0,
            Err(_) => EACCES,
        },
        None => match vfs.create_file(root, &path, Vec::new(), uid) {
            Ok(_) => 0,
            Err(e) => fs_error(e),
        },
    }
}

pub(super) fn sys_mkdir(path_ptr: u64) -> i64 {
    match path_arg(path_ptr) {
        Ok(path) => vfs_op(|v, uid| if v.exists(0, &path) { Err("already exists") } else { v.mkdir(0, &path, uid).map(|_| ()) }),
        Err(e) => e,
    }
}

pub(super) fn sys_unlink(path_ptr: u64) -> i64 {
    match path_arg(path_ptr) {
        Ok(path) => {
            super::linux::socket::unlink_hook(&path);
            vfs_op(|v, uid| v.remove(0, &path, uid))
        }
        Err(e) => e,
    }
}

pub(super) fn sys_rename(from_ptr: u64, to_ptr: u64) -> i64 {
    match (path_arg(from_ptr), path_arg(to_ptr)) {
        (Ok(from), Ok(to)) => vfs_op(|v, uid| v.rename(0, &from, &to, uid)),
        (Err(e), _) | (_, Err(e)) => e,
    }
}

pub(super) fn sys_chmod(path_ptr: u64, mode: u64) -> i64 {
    match path_arg(path_ptr) {
        Ok(path) => vfs_op(|v, uid| v.chmod(0, &path, uid, mode as u16)),
        Err(e) => e,
    }
}

pub(super) fn sys_chown(path_ptr: u64, owner: u64) -> i64 {
    match path_arg(path_ptr) {
        Ok(path) => vfs_op(|v, uid| v.chown(0, &path, uid, owner as u32)),
        Err(e) => e,
    }
}

pub(super) fn sys_stat(path_ptr: u64, buf: u64) -> i64 {
    let path = match path_arg(path_ptr) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let stat = {
        let guard = fs::VFS.lock();
        let Some(vfs) = guard.as_ref() else {
            return ENODEV;
        };
        match vfs.resolve(0, &path) {
            Some(id) => vfs.stat(id),
            None => return ENOENT,
        }
    };
    let mut raw = [0u8; 24];
    raw[0..4].copy_from_slice(&stat.kind.to_le_bytes());
    raw[4..8].copy_from_slice(&(stat.mode as u32).to_le_bytes());
    raw[8..12].copy_from_slice(&stat.owner.to_le_bytes());
    raw[12..16].copy_from_slice(&(stat.dev as u32).to_le_bytes());
    raw[16..24].copy_from_slice(&stat.size.to_le_bytes());
    let r = copy_out(buf, 24, &raw);
    if r < 0 { r } else { 0 }
}

pub(super) fn sys_fbmap(buf: u64) -> i64 {
    let out = match user_slice(buf, 24) {
        Ok(o) => o,
        Err(e) => return e,
    };
    let (pid, vt, _, _) = current_identity();
    match task::display::map(pid, vt) {
        Ok(fb) => {
            out[0..8].copy_from_slice(&fb.addr.to_le_bytes());
            out[8..12].copy_from_slice(&fb.pitch.to_le_bytes());
            out[12..16].copy_from_slice(&fb.width.to_le_bytes());
            out[16..20].copy_from_slice(&fb.height.to_le_bytes());
            out[20..24].copy_from_slice(&(fb.bpp as u32).to_le_bytes());
            0
        }
        Err(e) => e,
    }
}

fn gpu_allowed() -> bool {
    task::display::owner() == Some(task::current_pid())
}

fn unpack(value: u64) -> (u32, u32) {
    ((value >> 32) as u32, value as u32)
}

pub(super) fn sys_gpu_flush(at: u64, size: u64) -> i64 {
    if !gpu_allowed() {
        return EACCES;
    }
    let (x, y) = unpack(at);
    let (w, h) = unpack(size);
    crate::drivers::video::gpu::flush(x, y, w, h) as i64
}

pub(super) fn sys_gpu_fill(at: u64, size: u64, color: u64) -> i64 {
    if !gpu_allowed() {
        return EACCES;
    }
    let (x, y) = unpack(at);
    let (w, h) = unpack(size);
    crate::drivers::video::gpu::fill(x, y, w, h, color as u32) as i64
}

pub(super) fn sys_gpu_copy(src: u64, dst: u64, size: u64) -> i64 {
    if !gpu_allowed() {
        return EACCES;
    }
    let (sx, sy) = unpack(src);
    let (dx, dy) = unpack(dst);
    let (w, h) = unpack(size);
    crate::drivers::video::gpu::copy(sx, sy, dx, dy, w, h) as i64
}

pub(super) fn sys_gpu_cursor_set(buf: u64, size: u64, hot: u64) -> i64 {
    if !gpu_allowed() {
        return EACCES;
    }
    let (w, h) = unpack(size);
    let (hx, hy) = unpack(hot);
    if w == 0 || h == 0 || w > crate::drivers::video::gpu::CURSOR_MAX || h > crate::drivers::video::gpu::CURSOR_MAX {
        return EINVAL;
    }
    let bytes = match user_slice(buf, w as u64 * h as u64 * 4) {
        Ok(b) => b,
        Err(e) => return e,
    };
    let mut pixels = alloc::vec![0u32; (w * h) as usize];
    for (dst, chunk) in pixels.iter_mut().zip(bytes.chunks_exact(4)) {
        *dst = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
    }
    crate::drivers::video::gpu::cursor_set(&pixels, w, h, hx, hy) as i64
}

pub(super) fn sys_gpu_cursor(at: u64, hide: bool) -> i64 {
    if !gpu_allowed() {
        return EACCES;
    }
    if hide {
        return crate::drivers::video::gpu::cursor_hide() as i64;
    }
    let (x, y) = unpack(at);
    crate::drivers::video::gpu::cursor_move(x as i32, y as i32) as i64
}

pub(super) fn sys_mouse(buf: u64) -> i64 {
    let out = match user_slice(buf, 16) {
        Ok(o) => o,
        Err(e) => return e,
    };
    let (pid, vt, _, _) = current_identity();
    let mut state = crate::drivers::input::mouse::take_state();
    if !crate::vt::input_allowed(pid, vt) {
        state.buttons = 0;
        state.wheel = 0;
        state.presses = [0; 3];
    }
    out[0..4].copy_from_slice(&state.x.to_le_bytes());
    out[4..8].copy_from_slice(&state.y.to_le_bytes());
    let packed = state.buttons as u32 | (state.presses[0].min(15) as u32) << 8 | (state.presses[1].min(15) as u32) << 12 | (state.presses[2].min(15) as u32) << 16;
    out[8..12].copy_from_slice(&packed.to_le_bytes());
    out[12..16].copy_from_slice(&state.wheel.to_le_bytes());
    if crate::drivers::input::mouse::present() { 0 } else { ENODEV }
}

pub(super) fn sys_wait_event(mask: u64, timeout_ms: i64) -> i64 {
    let deadline = if timeout_ms < 0 { None } else { Some(task::ticks() + task::ms_to_ticks(timeout_ms as u64)) };
    let stdin_pipe = match read_target(0) {
        Target::PipeRead(id) => Some(id),
        _ => None,
    };
    loop {
        let seq = task::input_seq();
        let (pid, vt, _, _) = current_identity();
        let mut ready = 0u64;
        if mask & EVENT_MESSAGE != 0 && ipc::has_message() {
            ready |= EVENT_MESSAGE;
        }
        if mask & EVENT_PIPE != 0 && pipe_fds_readable() {
            ready |= EVENT_PIPE;
        }
        if mask & EVENT_INPUT != 0 {
            if let Some(id) = stdin_pipe {
                if pipe::available(id) > 0 {
                    ready |= EVENT_INPUT;
                }
            } else if crate::vt::input_allowed(pid, vt) {
                let fresh = task::with_current(|t| {
                    let changed = t.seen_input != seq;
                    t.seen_input = seq;
                    changed
                });
                if fresh || keyboard::has_pending() {
                    ready |= EVENT_INPUT;
                }
            }
        }
        if ready != 0 {
            return ready as i64;
        }
        if timeout_ms == 0 {
            return 0;
        }
        let remaining = match deadline {
            Some(d) => {
                let now = task::ticks();
                if now >= d {
                    return 0;
                }
                Some(d - now)
            }
            None => Some(task::TICK_HZ),
        };
        let mut flags = 0;
        if mask & EVENT_INPUT != 0 {
            flags |= if stdin_pipe.is_some() { task::WAIT_PIPE } else { task::WAIT_INPUT };
        }
        if mask & EVENT_MESSAGE != 0 {
            flags |= task::WAIT_MSG;
        }
        if mask & EVENT_PIPE != 0 {
            flags |= task::WAIT_PIPE;
        }
        task::block(flags, remaining, seq);
        task::check_killed();
        crate::vt::service_pending();
    }
}

pub(super) fn spawn_env(flags: u64, header: u64) -> Result<Option<Vec<String>>, i64> {
    if flags & SPAWN_ENV == 0 {
        return Ok(None);
    }
    let raw = user_slice(header, 16)?;
    let ptr = u64::from_le_bytes(raw[0..8].try_into().unwrap());
    let len = u64::from_le_bytes(raw[8..16].try_into().unwrap());
    let env: Vec<String> = user_args(ptr, len)?.into_iter().filter(|e| e.find('=').map(|i| i > 0).unwrap_or(false)).collect();
    if env.len() > MAX_ENV_ENTRIES {
        return Err(EINVAL);
    }
    Ok(Some(env))
}

pub(super) fn sys_spawn(path_ptr: u64, path_len: u64, args_ptr: u64, packed: u64, stdio: u64, env_header: u64) -> i64 {
    let args_len = packed & 0xFFFF_FFFF;
    let flags = packed >> 32;
    let path = match user_string(path_ptr, path_len) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let args = match user_args(args_ptr, args_len) {
        Ok(a) => a,
        Err(e) => return e,
    };
    let env = match spawn_env(flags, env_header) {
        Ok(env) => env,
        Err(e) => return e,
    };
    spawn_from_user(&path, &args, env, flags, stdio)
}

pub(super) fn resolve_program(cwd: &str, name: &str) -> Option<String> {
    let guard = fs::VFS.lock();
    let vfs = guard.as_ref()?;
    let root = vfs.root_id();
    let candidates: Vec<String> = if name.contains('/') {
        alloc::vec![fs::absolute(cwd, name)]
    } else {
        alloc::vec![format!("/usr/bin/{}", name), format!("/bin/{}", name), format!("/sbin/{}", name)]
    };
    candidates.into_iter().find(|p| vfs.resolve(root, p).map(|id| !vfs.is_dir(id)).unwrap_or(false))
}

pub(super) fn has_root_ticket() -> bool {
    task::with_current(|t| t.uid == 0 || t.root_ticket_until > task::ticks())
}

pub(super) fn spawn_from_user(program: &str, args: &[String], env: Option<Vec<String>>, flags: u64, stdio: u64) -> i64 {
    let (pid, vt, uid, cwd) = current_identity();
    let Some(path) = resolve_program(&cwd, program) else {
        return ENOENT;
    };
    let (child_uid, child_ruid) = if flags & SPAWN_ROOT != 0 {
        if !has_root_ticket() {
            return EPERM;
        }
        (0, 0)
    } else {
        (uid, task::with_current(|t| t.ruid))
    };
    let env = match env {
        Some(env) => env,
        None => {
            let inherited = task::with_current(|t| t.env.clone());
            if inherited.is_empty() { task::elf::default_env(child_uid) } else { inherited }
        }
    };
    let child = match task::elf::spawn(&path, args, &env, pid, vt, child_uid, child_ruid, &cwd) {
        Ok(child) => child,
        Err("out of memory") => return ENOMEM,
        Err("permission denied") => return EACCES,
        Err("no such file") | Err("no filesystem mounted") | Err("interpreter not found") => return ENOENT,
        Err("is a directory") => return EISDIR,
        Err(e) => {
            crate::debug_println!("spawn: {}: {}", path, e);
            return ENOEXEC;
        }
    };
    for slot in 0..4usize {
        let wanted = (stdio >> (slot * 16)) & 0xFFFF;
        if wanted == 0 {
            continue;
        }
        let fd = (wanted - 1) as usize;
        let file = task::with_current(|t| t.fds.get(fd).and_then(|f| f.as_ref()).map(|f| f.duplicate()));
        let old = task::with_task(child, |t| core::mem::replace(&mut t.fds[slot], file)).flatten();
        if let Some(old) = old {
            old.release();
        }
    }
    if flags & SPAWN_DETACH != 0 {
        task::detach(child);
    }
    if flags & SPAWN_FOREGROUND != 0 {
        match target(0) {
            Target::Console => {
                if crate::vt::input_owner(vt) == pid {
                    crate::vt::set_input_owner(vt, child);
                }
            }
            Target::PipeRead(id) | Target::Pty(id, _) => pipe::set_foreground(id, child),
            _ => {}
        }
    }
    child as i64
}

pub(super) fn sys_set_foreground(target_pid: u64) -> i64 {
    let (pid, vt, _, _) = current_identity();
    let target_pid = if target_pid == 0 { pid } else { target_pid as Pid };
    match target(0) {
        Target::Console => {
            let owner = crate::vt::input_owner(vt);
            let owner_parent = task::with_task(owner, |t| t.parent).unwrap_or(0);
            if owner == pid || owner == 0 || owner_parent == pid || !task::exists(owner) {
                crate::vt::set_input_owner(vt, target_pid);
                0
            } else {
                EPERM
            }
        }
        Target::PipeRead(id) | Target::Pty(id, _) => {
            pipe::set_foreground(id, if target_pid == pid { 0 } else { target_pid });
            0
        }
        _ => ENOTTY,
    }
}

pub(super) fn sys_waitpid(child: u64, flags: u64) -> i64 {
    let parent = task::current_pid();
    loop {
        match task::try_reap(parent, child as Pid) {
            task::WaitResult::Exited(code) => return (code as u32) as i64,
            task::WaitResult::NoChild => return ECHILD,
            task::WaitResult::Running => {
                if flags & 1 != 0 {
                    return EAGAIN;
                }
                task::block(task::WAIT_CHILD, Some(task::TICK_HZ / 5), task::input_seq());
                task::check_killed();
            }
        }
    }
}

pub(super) fn sys_kill(target_pid: u64) -> i64 {
    let (uid, ruid) = task::with_current(|t| (t.uid, t.ruid));
    if uid != 0 {
        let target_uid = task::with_task(target_pid as Pid, |t| (t.uid, t.ruid));
        match target_uid {
            Some((u, r)) if u == uid || r == ruid => {}
            Some(_) => return EACCES,
            None => return ESRCH,
        }
    }
    if task::kill(target_pid as Pid, 143) { 0 } else { ESRCH }
}

pub(super) fn sys_proc_list(buf: u64, len: u64) -> i64 {
    let mut text = String::new();
    let owners: Vec<(Pid, u32)> = task::with_tasks(|tasks| tasks.values().map(|t| (t.pid, t.uid)).collect());
    for info in task::list() {
        let state = match info.state {
            task::State::Runnable => "R",
            task::State::Blocked => "S",
            task::State::Zombie => "Z",
        };
        let uid = owners.iter().find(|(p, _)| *p == info.pid).map(|(_, u)| *u).unwrap_or(0);
        text.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            info.pid,
            info.parent,
            state,
            info.vt + 1,
            if info.kernel_thread { format!("[{}]", info.name) } else { info.name.clone() },
            info.memory / 1024,
            info.cpu_ticks * 1000 / task::TICK_HZ,
            uid,
            info.cpu.map(|c| c as i64).unwrap_or(-1),
            if info.kernel_thread { "kernel" } else { info.abi.name() }
        ));
    }
    copy_out(buf, len, text.as_bytes())
}

pub(super) fn sys_sysinfo(buf: u64, len: u64) -> i64 {
    let extended = len == 64;
    let out = match user_slice(buf, if extended { 64 } else { 56 }) {
        Ok(o) => o,
        Err(e) => return e,
    };
    let (free, total) = crate::memory::frame::memory_info();
    let (heap_free, heap_total) = crate::memory::heap_stats();
    let values: [u64; 7] = [
        total as u64,
        free as u64,
        heap_total as u64,
        heap_free as u64,
        task::uptime_ms(),
        task::list().len() as u64,
        crate::drivers::rtc::boot_epoch() + task::uptime_ms() / 1000,
    ];
    for (i, v) in values.iter().enumerate() {
        out[i * 8..i * 8 + 8].copy_from_slice(&v.to_le_bytes());
    }
    if extended {
        out[56..64].copy_from_slice(&(fs::file_cache_bytes() as u64).to_le_bytes());
    }
    0
}

pub(super) fn sys_msg_recv(buf: u64, len: u64, sender_ptr: u64, timeout_ms: i64) -> i64 {
    let deadline = if timeout_ms < 0 { None } else { Some(task::ticks() + task::ms_to_ticks(timeout_ms as u64)) };
    loop {
        if let Some(message) = ipc::take_message() {
            if sender_ptr != 0 {
                if let Ok(out) = user_slice(sender_ptr, 4) {
                    out.copy_from_slice(&message.sender.to_le_bytes());
                }
            }
            return copy_out(buf, len, &message.data);
        }
        if timeout_ms == 0 {
            return EAGAIN;
        }
        let remaining = match deadline {
            Some(d) => {
                let now = task::ticks();
                if now >= d {
                    return EAGAIN;
                }
                Some(d - now)
            }
            None => Some(task::TICK_HZ),
        };
        task::block(task::WAIT_MSG, remaining, task::input_seq());
        task::check_killed();
    }
}

pub(super) fn sys_cmd_register(name_ptr: u64, name_len: u64, target_ptr: u64, packed: u64) -> i64 {
    let name = match user_string(name_ptr, name_len) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let parts = match user_args(target_ptr, packed & 0xFFFF_FFFF) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let Some((path, args)) = parts.split_first() else {
        return EINVAL;
    };
    let (_, _, uid, cwd) = current_identity();
    let path = fs::absolute(&cwd, path);
    match registry::register(&name, &path, args.to_vec(), uid) {
        Ok(()) => 0,
        Err("command belongs to another user") => EACCES,
        Err("target does not exist") => ENOENT,
        Err(_) => EINVAL,
    }
}

pub(super) fn sys_cmd_run(name_ptr: u64, name_len: u64, args_ptr: u64, packed: u64) -> i64 {
    let name = match user_string(name_ptr, name_len) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let extra = match user_args(args_ptr, packed & 0xFFFF_FFFF) {
        Ok(a) => a,
        Err(e) => return e,
    };
    let Some(command) = registry::resolve(&name) else {
        return ENOENT;
    };
    let mut args = command.args.clone();
    args.extend(extra);
    spawn_from_user(&command.path, &args, None, (packed >> 32) & !SPAWN_ROOT, 0)
}

pub(super) fn sys_readdir(path_ptr: u64, path_len: u64, buf: u64, len: u64) -> i64 {
    let path = match user_string(path_ptr, path_len) {
        Ok(p) => absolute(&p),
        Err(e) => return e,
    };
    let mut text = String::new();
    {
        let guard = fs::VFS.lock();
        let Some(vfs) = guard.as_ref() else {
            return ENODEV;
        };
        let entries = match vfs.list_detailed(vfs.root_id(), &path) {
            Ok(entries) => entries,
            Err("not a directory") => return ENOTDIR,
            Err(_) => return ENOENT,
        };
        for (name, id) in entries {
            let stat = vfs.stat(id);
            let kind = match stat.kind {
                fs::KIND_DIR => 'd',
                fs::KIND_DEVICE => 'c',
                fs::KIND_PROC => 'p',
                _ => 'f',
            };
            text.push_str(&format!("{}\t{}\t{}\t{}\t{}\n", name, kind, stat.size, stat.mode, stat.owner));
        }
    }
    copy_out(buf, len, text.as_bytes())
}

pub(super) fn disk_listing() -> String {
    let mut text = String::new();
    let infos = block::volume_infos();
    for (index, disk) in block::disks().iter().enumerate() {
        let whole = infos.iter().find(|v| v.volume.disk == index && v.partition.is_none());
        let probe = whole.and_then(|w| w.probe.clone());
        text.push_str(&format!(
            "disk\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            disk.name,
            disk.sectors,
            disk.bus,
            disk.model,
            probe.as_ref().map(|p| p.kind).unwrap_or(""),
            probe.as_ref().map(|p| p.label.as_str()).unwrap_or(""),
            probe.as_ref().map(|p| p.uuid.as_str()).unwrap_or(""),
            fs::hextfs::mount_path_of(index, 0).unwrap_or_default()
        ));
        for info in infos.iter().filter(|v| v.volume.disk == index) {
            let Some(part) = &info.partition else {
                continue;
            };
            text.push_str(&format!(
                "part\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                part.name,
                disk.name,
                part.start,
                part.sectors,
                part.kind,
                info.probe.as_ref().map(|p| p.kind).unwrap_or(""),
                info.probe.as_ref().map(|p| p.label.as_str()).unwrap_or(""),
                info.probe.as_ref().map(|p| p.uuid.as_str()).unwrap_or(""),
                fs::hextfs::mount_path_of(index, part.start).unwrap_or_default(),
                if part.bootable { "boot" } else { "" }
            ));
        }
    }
    text
}

pub(super) fn require_root_or_console() -> Result<(), i64> {
    let uid = euid();
    if uid == 0 {
        return Ok(());
    }
    let owner_uid = crate::task::display::owner().and_then(|pid| task::with_task(pid, |t| t.ruid));
    let vt = task::current_vt();
    if owner_uid == Some(uid) || crate::vt::foreground() == vt { Ok(()) } else { Err(EPERM) }
}

pub(super) fn require_root() -> Result<(), i64> {
    if euid() == 0 { Ok(()) } else { Err(EPERM) }
}

pub(super) fn sys_disk_io(name_ptr: u64, lba: u64, buf: u64, sectors: u64, write: u64) -> i64 {
    if let Err(e) = require_root() {
        return e;
    }
    if sectors == 0 || sectors > 4096 {
        return EINVAL;
    }
    let name = match user_cstr(name_ptr) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let Some(volume) = block::find_volume(&name) else {
        return ENODEV;
    };
    if lba + sectors > volume.sectors {
        return EINVAL;
    }
    let bytes = (sectors * 512) as usize;
    let user = match user_slice(buf, bytes as u64) {
        Ok(b) => b,
        Err(e) => return e,
    };
    let absolute_lba = volume.offset + lba;
    if write != 0 {
        if fs::hextfs::overlaps_mounted(volume.disk, absolute_lba, sectors) {
            return EBUSY;
        }
        let data = user.to_vec();
        if block::write(volume.disk, absolute_lba, &data).is_err() {
            return EIO;
        }
        let _ = block::flush(volume.disk);
        0
    } else {
        let mut data = alloc::vec![0u8; bytes];
        if block::read(volume.disk, absolute_lba, &mut data).is_err() {
            return EIO;
        }
        match user_slice(buf, bytes as u64) {
            Ok(out) => {
                out.copy_from_slice(&data);
                0
            }
            Err(e) => e,
        }
    }
}

pub(super) fn sys_mkfs(dev_ptr: u64, label_ptr: u64) -> i64 {
    if let Err(e) = require_root() {
        return e;
    }
    let (dev, label) = match (user_cstr(dev_ptr), user_cstr(label_ptr)) {
        (Ok(d), Ok(l)) => (d, l),
        (Err(e), _) | (_, Err(e)) => return e,
    };
    match fs::hextfs::mkfs(&dev, &label) {
        Ok(msg) => {
            crate::drivers::klog::log(&format!("mkfs: {}", msg));
            0
        }
        Err("no such block device") => ENODEV,
        Err("the device is mounted") => EBUSY,
        Err(_) => EIO,
    }
}

pub(super) fn sys_mount(dev_ptr: u64, path_ptr: u64) -> i64 {
    if let Err(e) = require_root() {
        return e;
    }
    let (dev, path) = match (user_cstr(dev_ptr), path_arg(path_ptr)) {
        (Ok(d), Ok(p)) => (d, p),
        (Err(e), _) | (_, Err(e)) => return e,
    };
    match fs::hextfs::mount(&dev, &path) {
        Ok(msg) => {
            crate::drivers::klog::log(&format!("mount: {}", msg));
            0
        }
        Err("no such block device") => ENODEV,
        Err("the device is already mounted") | Err("something is already mounted there") => EBUSY,
        Err("mount point does not exist") => ENOENT,
        Err("mount point must be a directory other than /") => ENOTDIR,
        Err("no hext filesystem on the device") => EINVAL,
        Err(_) => EIO,
    }
}

pub(super) fn sys_umount(path_ptr: u64) -> i64 {
    if let Err(e) = require_root() {
        return e;
    }
    let path = match path_arg(path_ptr) {
        Ok(p) => p,
        Err(e) => return e,
    };
    match fs::hextfs::umount(&path) {
        Ok(()) => 0,
        Err("not mounted") => EINVAL,
        Err(_) => EBUSY,
    }
}

pub(super) fn sys_auth(user_ptr: u64, pass_ptr: u64) -> i64 {
    let (user, password) = match (user_cstr(user_ptr), user_cstr(pass_ptr)) {
        (Ok(u), Ok(p)) => (u, p),
        (Err(e), _) | (_, Err(e)) => return e,
    };
    crate::users::reload();
    if !crate::users::verify_password(&user, &password) {
        sleep_ms(800);
        return EACCES;
    }
    let ruid = task::with_current(|t| t.ruid);
    let own = crate::users::find_by_name(&user).map(|r| r.uid == ruid).unwrap_or(false);
    let granted = user == "root" || (own && crate::users::is_sudoer(&user));
    if granted {
        task::with_current(|t| t.root_ticket_until = task::ticks() + ROOT_TICKET_TICKS);
        0
    } else {
        1
    }
}

pub(super) fn sys_power(mode: u64) -> i64 {
    if let Err(e) = require_root() {
        return e;
    }
    fs::sync();
    crate::module::suspend_all();
    use crate::arch::disable_interrupts;
    let vt = task::current_vt();
    let message: &[u8] = match mode {
        1 => b"\n\x1b[93mRebooting...\x1b[0m\n",
        2 => b"\n\x1b[93mPowering off...\x1b[0m\n",
        _ => b"\n\x1b[93mSystem halted. It is now safe to turn off the computer.\x1b[0m\n",
    };
    if crate::task::display::owner().is_some() {
        let owner = crate::task::display::owner().unwrap();
        crate::task::display::release(owner);
    }
    TEXT_CONSOLE.lock().write_bytes_to(vt, message);
    sleep_ms(300);
    disable_interrupts();
    match mode {
        1 => crate::arch::platform::reset(),
        2 => crate::arch::platform::poweroff(),
        _ => {}
    }
    loop {
        crate::arch::hlt();
    }
}
