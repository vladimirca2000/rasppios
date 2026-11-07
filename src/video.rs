//! Inicialização do framebuffer RGB565 pelo mailbox de propriedades.

use core::mem::size_of;
use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};

use crate::{mmu, uart};

const MBOX_READ: usize = 0x3F00_B880;
const MBOX_STATUS: usize = 0x3F00_B898;
const MBOX_WRITE: usize = 0x3F00_B8A0;
const MBOX_FULL: u32 = 1 << 31;
const MBOX_EMPTY: u32 = 1 << 30;
const MBOX_CHANNEL: u32 = 8;
const BUS_ALIAS: u32 = 0xC000_0000;
const RAM_END: u64 = 0x3C00_0000;
const QEMU_FRAMEBUFFER_END: u64 = 0x3F00_0000;
const CACHE_LINE: usize = 64;

const TAG_PHYSICAL_SIZE: u32 = 0x0004_8003;
const TAG_VIRTUAL_SIZE: u32 = 0x0004_8004;
const TAG_DEPTH: u32 = 0x0004_8005;
const TAG_ALLOCATE_BUFFER: u32 = 0x0004_0001;
const TAG_PITCH: u32 = 0x0004_0008;

#[repr(C, align(16))]
struct FramebufferMessage {
    words: [u32; 35],
}

static mut MESSAGE: FramebufferMessage = FramebufferMessage { words: [0; 35] };
static mut AVAILABLE: bool = false;
static mut PITCH: u32 = 0;

pub fn init() -> bool {
    let Some((start, size, pitch)) = negotiate() else {
        return false;
    };
    if !mmu::map_framebuffer(start, size) {
        return false;
    }
    unsafe {
        write_volatile(addr_of_mut!(PITCH), pitch);
        write_volatile(addr_of_mut!(AVAILABLE), true);
    }
    let _ = available();
    uart::write_fmt(format_args!("video: 640x480 rgb565 stride {pitch}\r\n"));
    true
}

pub fn available() -> bool {
    unsafe { read_volatile(addr_of!(AVAILABLE)) }
}

#[allow(dead_code)]
pub fn exercise() {
    let _ = available();
}

fn negotiate() -> Option<(u64, usize, u32)> {
    unsafe {
        let words = addr_of_mut!(MESSAGE.words) as *mut u32;
        for index in 0..35 {
            write_volatile(words.add(index), 0);
        }
        write_volatile(words.add(0), 35 * 4);
        write_volatile(words.add(2), TAG_PHYSICAL_SIZE);
        write_volatile(words.add(3), 8);
        write_volatile(words.add(4), 8);
        write_volatile(words.add(5), 640);
        write_volatile(words.add(6), 480);
        write_volatile(words.add(7), TAG_VIRTUAL_SIZE);
        write_volatile(words.add(8), 8);
        write_volatile(words.add(9), 8);
        write_volatile(words.add(10), 640);
        write_volatile(words.add(11), 480);
        write_volatile(words.add(12), TAG_DEPTH);
        write_volatile(words.add(13), 4);
        write_volatile(words.add(14), 4);
        write_volatile(words.add(15), 16);
        write_volatile(words.add(16), TAG_ALLOCATE_BUFFER);
        write_volatile(words.add(17), 8);
        write_volatile(words.add(18), 8);
        write_volatile(words.add(19), 0);
        write_volatile(words.add(20), 0);
        write_volatile(words.add(21), TAG_PITCH);
        write_volatile(words.add(22), 4);
        write_volatile(words.add(23), 0);
        write_volatile(words.add(24), 0);
        write_volatile(words.add(25), 0);
    }
    dsb();
    cache_clean_message();

    if !wait(|status| status & MBOX_FULL == 0) {
        return None;
    }
    let message = addr_of!(MESSAGE) as usize;
    let bus = ((message as u32) | BUS_ALIAS) & !0xf;
    unsafe { write_volatile(MBOX_WRITE as *mut u32, bus | MBOX_CHANNEL) };
    dsb();

    if !wait(|status| status & MBOX_EMPTY == 0) {
        return None;
    }
    let reply = unsafe { read_volatile(MBOX_READ as *const u32) };
    dsb();
    if reply & 0xf != MBOX_CHANNEL {
        return None;
    }
    cache_invalidate_message();

    unsafe {
        let words = addr_of!(MESSAGE.words) as *const u32;
        let response_codes = [4usize, 9, 14, 18, 23];
        for index in response_codes {
            if read_volatile(words.add(index)) & (1 << 31) == 0 {
                return None;
            }
        }
        let width = read_volatile(words.add(5));
        let height = read_volatile(words.add(6));
        let virtual_width = read_volatile(words.add(10));
        let virtual_height = read_volatile(words.add(11));
        let depth = read_volatile(words.add(15));
        let buffer_bus = read_volatile(words.add(19));
        let buffer_size = read_volatile(words.add(20)) as usize;
        let pitch = read_volatile(words.add(24));
        if width != 640
            || height != 480
            || virtual_width != 640
            || virtual_height != 480
            || depth != 16
            || pitch < width * 2
            || buffer_bus & 0xf != 0
        {
            return None;
        }
        let required = (pitch as usize).checked_mul(height as usize)?;
        if buffer_size < required {
            return None;
        }
        let buffer_start = if buffer_bus & BUS_ALIAS == BUS_ALIAS {
            u64::from(buffer_bus - BUS_ALIAS)
        } else {
            u64::from(buffer_bus)
        };
        let buffer_end = buffer_start.checked_add(buffer_size as u64)?;
        let in_qemu_aperture =
            buffer_start >= RAM_END && buffer_end <= QEMU_FRAMEBUFFER_END;
        let in_arm_ram = buffer_end <= RAM_END;
        if !in_qemu_aperture && !in_arm_ram {
            return None;
        }
        Some((buffer_start, buffer_size, pitch))
    }
}

fn wait(ready: fn(u32) -> bool) -> bool {
    for _ in 0..1_000_000u32 {
        let status = unsafe { read_volatile(MBOX_STATUS as *const u32) };
        if ready(status) {
            return true;
        }
    }
    false
}

fn dsb() {
    unsafe { core::arch::asm!("dsb sy", options(nostack, preserves_flags)) };
}

fn cache_clean_message() {
    cache_maintain_message(false);
}

fn cache_invalidate_message() {
    cache_maintain_message(true);
}

fn cache_maintain_message(invalidate: bool) {
    let message_start = addr_of!(MESSAGE) as usize;
    let Some(message_end) = message_start.checked_add(size_of::<FramebufferMessage>()) else {
        return;
    };
    let Some(aligned_end) = message_end.checked_add(CACHE_LINE - 1) else {
        return;
    };
    let start = message_start & !(CACHE_LINE - 1);
    let end = aligned_end & !(CACHE_LINE - 1);
    let mut line = start;
    while line < end {
        unsafe {
            if invalidate {
                core::arch::asm!("dc ivac, {line}", line = in(reg) line, options(nostack));
            } else {
                core::arch::asm!("dc civac, {line}", line = in(reg) line, options(nostack));
            }
        }
        let Some(next) = line.checked_add(CACHE_LINE) else {
            break;
        };
        line = next;
    }
    dsb();
}
