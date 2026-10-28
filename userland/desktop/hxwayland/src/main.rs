#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec::Vec;
use hxstd::{entry, eprintln, sys, unix};
use hxwayland::{Bridge, IpcLink, Link};

fn main() -> i32 {
    let server = sys::service_lookup(hxproto::SERVICE);
    sys::service_register("hxwayland");
    if server < 0 {
        eprintln!("hxwayland: hxserver is not running (start it with startx)");
        return 1;
    }
    let mailbox = unix::mailbox_fd();
    if mailbox < 0 {
        eprintln!("hxwayland: no mailbox fd");
        return 1;
    }
    let mut bridge = match Bridge::start(IpcLink { server, mailbox: mailbox as u64 }) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("hxwayland: {}", e);
            return 1;
        }
    };
    loop {
        let timeout = bridge.timeout_ms().map(|t| t as i32).unwrap_or(1000);
        let mut fds = alloc_fds(&mut bridge);
        fds.push(unix::PollFd { fd: mailbox as i32, events: unix::POLLIN, revents: 0 });
        unix::poll(&mut fds, timeout);
        bridge.service(0);
        if !bridge.link.alive() {
            bridge.shutdown();
            return 0;
        }
    }
}

fn alloc_fds(bridge: &mut Bridge<IpcLink>) -> Vec<unix::PollFd> {
    bridge
        .fds()
        .into_iter()
        .map(|(fd, out)| unix::PollFd { fd, events: if out { unix::POLLIN | unix::POLLOUT } else { unix::POLLIN }, revents: 0 })
        .collect()
}

entry!(main);
