use hxproto::{Event, Request, MESSAGE_MAX};
use hxstd::sys;

use crate::compositor::window::Body;
use crate::Nook;

pub struct Embedded {
    pub nook: *mut Nook,
    pub me: i64,
    pub mailbox: u64,
}

impl hxwayland::Link for Embedded {
    fn request(&mut self, request: Request) {
        let nook = unsafe { &mut *self.nook };
        nook.handle_request(self.me, request);
    }

    fn receive(&mut self, timeout_ms: i64) -> Option<Event> {
        let mut buf = [0u8; MESSAGE_MAX];
        let len = sys::msg_recv_from(self.me, &mut buf, timeout_ms.max(0))?;
        Event::decode(&buf[..len])
    }

    fn alive(&self) -> bool {
        true
    }

    fn mailbox(&self) -> u64 {
        self.mailbox
    }

    fn direct(&self) -> bool {
        true
    }

    fn trusted_peer(&self, pid: u32) -> bool {
        unsafe { &*self.nook }.is_shell(pid as i64)
    }

    fn attach(&mut self, attach: hxwayland::Attach) {
        let nook = unsafe { &mut *self.nook };
        nook.attach_external(self.me, attach);
    }
}

impl Nook {
    pub fn attach_external(&mut self, sender: i64, a: hxwayland::Attach) {
        let Some(index) = self.core.index_of(a.window) else {
            return;
        };
        if self.core.windows[index].client_pid() != Some(sender) || !self.trusted_bridge(sender) {
            return;
        }
        let (w, h) = (a.width as i32, a.height as i32);
        let resized = match &mut self.core.windows[index].body {
            Body::Wayland { pixels, width, height, stride, opaque, .. } => {
                *pixels = a.pixels;
                *stride = a.stride as i32;
                *opaque = a.opaque;
                (*width, *height) != (w, h)
            }
            _ => return,
        };
        if resized {
            self.handle_request(sender, Request::Attach { window: a.window, shm: 0, width: a.width, height: a.height });
        }
        let (x, y, dw, dh) = if resized { (0, 0, a.width, a.height) } else { a.damage };
        self.handle_request(sender, Request::Present { window: a.window, x, y, width: dw, height: dh });
    }
}
