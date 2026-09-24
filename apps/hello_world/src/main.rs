#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec;
use hamix_std::{entry, env, println, sys, unix};

fn check(ok: bool, what: &str, failures: &mut u32) {
    println!("{} {}", if ok { "  ok  " } else { "  FAIL" }, what);
    if !ok {
        *failures += 1;
    }
}

fn unix_client(path: &str) -> i32 {
    let mut failures = 0;
    let mfd = unix::memfd_create("native-buffer");
    check(mfd >= 0, "memfd_create", &mut failures);
    let mfd = mfd as u64;
    check(unix::ftruncate(mfd, 4096) == 0, "ftruncate", &mut failures);
    let map = match unix::map_shared(mfd, 4096, 0) {
        Ok(p) => p,
        Err(e) => {
            println!("map_shared failed: {}", e);
            return 1;
        }
    };
    let greeting = b"hello from client\0";
    unsafe { core::ptr::copy_nonoverlapping(greeting.as_ptr(), map, greeting.len()) };
    let (pipe_r, pipe_w) = match sys::pipe() {
        Ok(p) => p,
        Err(e) => {
            println!("pipe failed: {}", e);
            return 1;
        }
    };
    let mut sock = -1;
    for _ in 0..50 {
        sock = unix::connect_to(path, unix::SOCK_STREAM);
        if sock >= 0 {
            break;
        }
        sys::sleep_ms(100);
    }
    check(sock >= 0, "native connect", &mut failures);
    if sock < 0 {
        return 1;
    }
    let sock = sock as u64;
    let cred = unix::peer_cred(sock);
    check(cred.map(|c| c.pid > 0).unwrap_or(false), "peer credentials", &mut failures);
    check(unix::send_with_fds(sock, b"ping", &[mfd, pipe_w], 0) == 4, "send ping with memfd and pipe", &mut failures);
    sys::close(pipe_w);
    let mut buf = [0u8; 64];
    let reply = unix::recv_with_fds(sock, &mut buf, 0);
    let back = match &reply {
        Ok((n, fds)) => {
            check(&buf[..*n] == b"pong" && fds.len() == 1, "received pong with 1 fd", &mut failures);
            fds.first().copied()
        }
        Err(e) => {
            println!("recv failed: {}", e);
            failures += 1;
            None
        }
    };
    let shared = unsafe { core::slice::from_raw_parts(map, 16) };
    check(&shared[..16] == b"server was here\0", "shared mapping shows server write", &mut failures);
    let mut pipe_buf = [0u8; 16];
    check(sys::read(pipe_r, &mut pipe_buf) == 8 && &pipe_buf[..8] == b"via pipe", "read from pipe written by server", &mut failures);
    if let Some(fd) = back {
        let mut probe = [0u8; 15];
        check(unix::pread(fd, &mut probe, 0) == 15 && &probe == b"server was here", "returned fd reads same memory", &mut failures);
    }
    let ep = unix::epoll_create();
    let mailbox = unix::mailbox_fd();
    check(ep >= 0 && mailbox >= 0, "epoll and mailbox fds", &mut failures);
    unix::epoll_ctl(ep as u64, unix::EPOLL_CTL_ADD, mailbox as u64, unix::EPOLLIN, 7);
    let mut events = [(0u32, 0u64); 4];
    check(unix::epoll_wait(ep as u64, &mut events, 0) == 0, "mailbox idle", &mut failures);
    sys::msg_send(sys::getpid(), b"wake");
    check(unix::epoll_wait(ep as u64, &mut events, 1000) == 1 && events[0].1 == 7, "epoll wakes on mailbox message", &mut failures);
    sys::close(sock);
    unix::unmap(map, 4096);
    println!("{}", if failures == 0 { "NATIVE CLIENT OK" } else { "NATIVE CLIENT FAILED" });
    (failures != 0) as i32
}

fn main() -> i32 {
    let args = env::args();
    if args.len() == 3 && args[1] == "unix" {
        return unix_client(&args[2]);
    }

    println!("HamixOS userspace: hello from Rust + hamix_std!");

    let numbers = vec![1u32, 2, 3, 4, 5];
    let sum: u32 = numbers.iter().sum();
    println!("computed on the heap (brk syscall): sum = {}", sum);

    println!("uid={} euid={} pid={}", sys::getuid(), sys::geteuid(), sys::getpid());

    0
}

entry!(main);
