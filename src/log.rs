//! Registro fixo de eventos para diagnóstico sem alocação.

use core::ptr::{addr_of, addr_of_mut};

use crate::uart;

const CAPACITY: usize = 8;
pub const EVENT_LEN: usize = 31;
pub const DISPLAY_EVENTS: usize = 8;

#[derive(Clone, Copy)]
struct State {
    entries: [[u8; EVENT_LEN]; CAPACITY],
    lengths: [usize; CAPACITY],
    next: usize,
    count: usize,
    sequence: u32,
}

static mut STATE: State = State {
    entries: [[0; EVENT_LEN]; CAPACITY],
    lengths: [0; CAPACITY],
    next: 0,
    count: 0,
    sequence: 0,
};

pub fn info(event: &'static str) {
    record(b"I", event);
}

pub fn warn(event: &'static str) {
    record(b"W", event);
}

pub fn error(event: &'static str) {
    record(b"E", event);
}

pub fn print() {
    let state = unsafe { core::ptr::read_volatile(addr_of!(STATE)) };
    uart::puts("logs:\r\n");
    let first = if state.count == CAPACITY {
        state.next
    } else {
        0
    };
    for offset in 0..state.count {
        let index = (first + offset) % CAPACITY;
        uart::write_fmt(format_args!(
            "log[{}]: {}\r\n",
            offset,
            core::str::from_utf8(&state.entries[index][..state.lengths[index]])
                .unwrap_or("invalido"),
        ));
    }
}

pub fn print_limits() {
    uart::write_fmt(format_args!(
        "limites: blocos 64..82 nomes {} bytes arquivos {} bytes logs {}x{}\r\n",
        crate::limits::MAX_NAME,
        crate::limits::MAX_FILE,
        CAPACITY,
        EVENT_LEN,
    ));
}

pub fn snapshot(dest: &mut [[u8; EVENT_LEN]; DISPLAY_EVENTS]) -> [usize; DISPLAY_EVENTS] {
    let state = unsafe { core::ptr::read_volatile(addr_of!(STATE)) };
    let mut lengths = [0; DISPLAY_EVENTS];
    let count = state.count.min(DISPLAY_EVENTS);
    let first = if state.count == CAPACITY {
        (state.next + CAPACITY - count) % CAPACITY
    } else {
        state.count - count
    };
    for offset in 0..count {
        let index = (first + offset) % CAPACITY;
        let length = state.lengths[index];
        dest[offset][..length].copy_from_slice(&state.entries[index][..length]);
        lengths[offset] = length;
    }
    lengths
}

pub fn generation() -> usize {
    unsafe { core::ptr::read_volatile(addr_of!(STATE.sequence)) as usize }
}

fn record(level: &[u8; 1], event: &str) {
    let state = unsafe { &mut *addr_of_mut!(STATE) };
    let index = state.next;
    state.entries[index] = [0; EVENT_LEN];
    let mut length = 0;
    state.entries[index][length] = level[0];
    length += 1;
    if length < EVENT_LEN {
        state.entries[index][length] = b':';
        length += 1;
    }
    if length < EVENT_LEN {
        state.entries[index][length] = b' ';
        length += 1;
    }
    let bytes = event.as_bytes();
    let copy = (EVENT_LEN - length).min(bytes.len());
    state.entries[index][length..length + copy].copy_from_slice(&bytes[..copy]);
    state.lengths[index] = length + copy;
    state.next = (index + 1) % CAPACITY;
    state.count = (state.count + 1).min(CAPACITY);
    state.sequence = state.sequence.wrapping_add(1);
    uart::write_fmt(format_args!(
        "log {}: {}\r\n",
        core::str::from_utf8(level).unwrap_or("?"),
        event,
    ));
}
