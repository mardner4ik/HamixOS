#![no_std]

extern crate alloc;

use hxproto::{Event, Request, MESSAGE_MAX};
use hxstd::sys;

#[path = "../../hxserver/src/frontend/wayland/protocol.rs"]
mod protocol;

pub use protocol::{Attach, Bridge, Link};

pub struct IpcLink {
    pub server: i64,
    pub mailbox: u64,
}

impl Link for IpcLink {
    fn request(&mut self, request: Request) {
        let mut buf = [0u8; MESSAGE_MAX];
        let n = request.encode(&mut buf);
        sys::msg_send(self.server, &buf[..n]);
    }

    fn receive(&mut self, timeout_ms: i64) -> Option<Event> {
        let mut buf = [0u8; MESSAGE_MAX];
        let deadline = sys::uptime_ms() + timeout_ms.max(0) as u64;
        loop {
            let left = deadline.saturating_sub(sys::uptime_ms()) as i64;
            let (sender, len) = sys::msg_recv(&mut buf, left)?;
            if sender == self.server {
                if let Some(event) = Event::decode(&buf[..len]) {
                    return Some(event);
                }
            }
            if sys::uptime_ms() >= deadline {
                return None;
            }
        }
    }

    fn alive(&self) -> bool {
        sys::proc_alive(self.server)
    }

    fn mailbox(&self) -> u64 {
        self.mailbox
    }
}
