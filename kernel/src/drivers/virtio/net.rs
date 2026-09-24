use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use super::{Queue, Transport};
use crate::net::dma::DmaRegion;
use crate::net::{Kind, NetDevice};

const F_MAC: u64 = 1 << 5;
const F_STATUS: u64 = 1 << 16;
const BUFFER: usize = 2048;
const RX_COUNT: usize = 64;
const TX_COUNT: usize = 32;

pub struct VirtioNet {
    name: String,
    transport: Box<dyn Transport>,
    rx: Queue,
    tx: Queue,
    rx_buffers: DmaRegion,
    tx_buffers: DmaRegion,
    tx_free: Vec<usize>,
    tx_slots: Vec<(u16, usize)>,
    header: usize,
    mac: [u8; 6],
    status: bool,
    received: u64,
    sent: u64,
}

unsafe impl Send for VirtioNet {}

impl VirtioNet {
    fn post_rx(&mut self, slot: usize) {
        let phys = self.rx_buffers.phys + (slot * BUFFER) as u64;
        self.rx.push(&[(phys, BUFFER as u32, true)]);
    }

    fn reclaim_tx(&mut self) {
        while let Some((head, _)) = self.tx.pop() {
            if let Some(at) = self.tx_slots.iter().position(|(h, _)| *h == head) {
                let (_, slot) = self.tx_slots.swap_remove(at);
                self.tx_free.push(slot);
            }
        }
    }
}

impl NetDevice for VirtioNet {
    fn name(&self) -> &str {
        &self.name
    }

    fn kind(&self) -> Kind {
        Kind::Ethernet
    }

    fn driver(&self) -> String {
        String::from("virtio-net")
    }

    fn mac(&self) -> [u8; 6] {
        self.mac
    }

    fn link_up(&mut self) -> bool {
        if !self.status {
            return true;
        }
        self.transport.config16(6) & 1 != 0
    }

    fn receive(&mut self) -> Option<Vec<u8>> {
        let (head, len) = self.rx.pop()?;
        let slot = self.rx_slot_of(head);
        let len = (len as usize).min(BUFFER);
        let frame = if len > self.header { self.rx_buffers.slice(slot * BUFFER + self.header, len - self.header).to_vec() } else { Vec::new() };
        self.post_rx(slot);
        self.rx.kick(self.transport.as_ref());
        self.received += 1;
        if frame.is_empty() { None } else { Some(frame) }
    }

    fn transmit(&mut self, frame: &[u8]) -> bool {
        self.reclaim_tx();
        if frame.len() + self.header > BUFFER {
            return false;
        }
        let Some(slot) = self.tx_free.pop() else {
            return false;
        };
        let buffer = self.tx_buffers.slice(slot * BUFFER, BUFFER);
        buffer[..self.header].fill(0);
        buffer[self.header..self.header + frame.len()].copy_from_slice(frame);
        let phys = self.tx_buffers.phys + (slot * BUFFER) as u64;
        match self.tx.push(&[(phys, (self.header + frame.len()) as u32, false)]) {
            Some(head) => {
                self.tx_slots.push((head, slot));
                self.tx.kick(self.transport.as_ref());
                self.sent += 1;
                true
            }
            None => {
                self.tx_free.push(slot);
                false
            }
        }
    }

    fn poll(&mut self) {
        self.transport.ack_interrupt();
        self.reclaim_tx();
    }

    fn counters(&self) -> (u64, u64) {
        (self.received, self.sent)
    }

    fn state_text(&self) -> String {
        format!("virtio-net, {} rx buffers", RX_COUNT)
    }
}

impl VirtioNet {
    fn rx_slot_of(&self, head: u16) -> usize {
        (self.rx.address_of(head).saturating_sub(self.rx_buffers.phys) as usize / BUFFER).min(RX_COUNT - 1)
    }
}

pub fn probe(found: &mut Vec<Box<dyn NetDevice>>) {
    for device in super::take(super::ID_NET) {
        let mut transport = device.transport;
        let Some(accepted) = super::negotiate(transport.as_mut(), F_MAC | F_STATUS) else {
            continue;
        };
        let (Some(rx), Some(tx)) = (Queue::new(transport.as_mut(), 0, RX_COUNT as u16), Queue::new(transport.as_mut(), 1, TX_COUNT as u16)) else {
            continue;
        };
        let (Some(rx_buffers), Some(tx_buffers)) = (DmaRegion::new(BUFFER * RX_COUNT), DmaRegion::new(BUFFER * TX_COUNT)) else {
            continue;
        };
        let mut mac = [0u8; 6];
        if accepted & F_MAC != 0 {
            for (i, byte) in mac.iter_mut().enumerate() {
                *byte = transport.config8(i);
            }
        } else {
            crate::random::fill(&mut mac);
            mac[0] = (mac[0] & 0xFE) | 0x02;
        }
        let header = if accepted & super::F_VERSION_1 != 0 { 12 } else { 10 };
        let index = found.len();
        let rx_size = rx.size as usize;
        let mut net = VirtioNet {
            name: format!("eth{}", index),
            transport,
            rx,
            tx,
            rx_buffers,
            tx_buffers,
            tx_free: (0..TX_COUNT).collect(),
            tx_slots: Vec::new(),
            header,
            mac,
            status: accepted & F_STATUS != 0,
            received: 0,
            sent: 0,
        };
        for slot in 0..RX_COUNT.min(rx_size) {
            net.post_rx(slot);
        }
        super::finish(net.transport.as_mut());
        net.rx.kick(net.transport.as_ref());
        found.push(Box::new(net));
    }
}
