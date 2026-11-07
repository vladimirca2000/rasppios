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
const REQUESTED_WIDTH: u32 = 640;
const REQUESTED_HEIGHT: u32 = 480;

pub const BLACK: u16 = 0x0000;
pub const NAVY: u16 = 0x000F;
pub const WHITE: u16 = 0xFFFF;
pub const RED: u16 = 0xF800;
pub const GREEN: u16 = 0x07E0;
pub const BLUE: u16 = 0x001F;

#[repr(C, align(16))]
struct FramebufferMessage {
    words: [u32; 35],
}

static mut MESSAGE: FramebufferMessage = FramebufferMessage { words: [0; 35] };
static mut AVAILABLE: bool = false;
static mut BUFFER: u64 = 0;
static mut PITCH: u32 = 0;
static mut WIDTH: u32 = 0;
static mut HEIGHT: u32 = 0;

pub fn init() -> bool {
    let Some((start, size, pitch, width, height)) = negotiate() else {
        return false;
    };
    if !mmu::map_framebuffer(start, size) {
        return false;
    }
    unsafe {
        write_volatile(addr_of_mut!(BUFFER), start);
        write_volatile(addr_of_mut!(PITCH), pitch);
        write_volatile(addr_of_mut!(WIDTH), width);
        write_volatile(addr_of_mut!(HEIGHT), height);
        write_volatile(addr_of_mut!(AVAILABLE), true);
    }
    let _ = available();
    uart::write_fmt(format_args!("video: 640x480 rgb565 stride {pitch}\r\n"));
    true
}

pub fn available() -> bool {
    unsafe { read_volatile(addr_of!(AVAILABLE)) }
}

/// Preenche a área visível inteira com uma cor RGB565.
pub fn clear(color: u16) {
    let (width, height) = dimensions();
    rect(0, 0, width, height, color);
}

/// Desenha um retângulo RGB565, recortando-o aos limites da tela.
pub fn rect(x: i32, y: i32, width: i32, height: i32, color: u16) {
    let Some((buffer, pitch, _, _)) = framebuffer() else {
        return;
    };
    let (screen_width, screen_height) = dimensions();
    let Some(x1) = x.checked_add(width) else {
        return;
    };
    let Some(y1) = y.checked_add(height) else {
        return;
    };
    let x0 = x.max(0).min(screen_width);
    let y0 = y.max(0).min(screen_height);
    let x1 = x1.max(0).min(screen_width);
    let y1 = y1.max(0).min(screen_height);
    if x0 >= x1 || y0 >= y1 {
        return;
    }

    for row in y0 as usize..y1 as usize {
        for column in x0 as usize..x1 as usize {
            let offset = row * pitch + column * size_of::<u16>();
            unsafe {
                write_volatile((buffer + offset) as *mut u16, color);
            }
        }
    }
}

/// Desenha texto com glifos monocromáticos 5×7 e avanço de seis pixels.
pub fn text(x: i32, y: i32, text: &str, color: u16, background: u16) {
    let Some((buffer, pitch, _, _)) = framebuffer() else {
        return;
    };
    let (screen_width, screen_height) = dimensions();
    let mut cursor_x = x;
    let mut cursor_y = y;
    for character in text.chars() {
        match character {
            '\r' => cursor_x = x,
            '\n' => {
                cursor_x = x;
                let Some(next_y) = cursor_y.checked_add(8) else {
                    break;
                };
                cursor_y = next_y;
                if cursor_y >= screen_height {
                    break;
                }
            }
            character => {
                let glyph = glyph(character);
                for (row, bits) in glyph.iter().copied().enumerate() {
                    let Some(pixel_y) = cursor_y.checked_add(row as i32) else {
                        continue;
                    };
                    if pixel_y < 0 || pixel_y >= screen_height {
                        continue;
                    }
                    for column in 0..5 {
                        let Some(pixel_x) = cursor_x.checked_add(column) else {
                            continue;
                        };
                        if pixel_x < 0 || pixel_x >= screen_width {
                            continue;
                        }
                        let pixel = if bits & (1 << (4 - column)) != 0 {
                            color
                        } else {
                            background
                        };
                        let offset = pixel_y as usize * pitch + pixel_x as usize * size_of::<u16>();
                        unsafe {
                            write_volatile((buffer + offset) as *mut u16, pixel);
                        }
                    }
                }
                let Some(next_x) = cursor_x.checked_add(6) else {
                    break;
                };
                cursor_x = next_x;
            }
        }
    }
}

pub fn exercise() -> bool {
    let Some((_, _, width, height)) = framebuffer() else {
        return false;
    };
    if width != REQUESTED_WIDTH || height != REQUESTED_HEIGHT {
        return false;
    }
    clear(NAVY);
    rect(0, 0, 640, 48, BLUE);
    rect(32, 96, 176, 112, RED);
    rect(232, 96, 176, 112, GREEN);
    rect(432, 96, 176, 112, WHITE);
    text(24, 16, "rasppios marco 18", WHITE, BLUE);
    text(48, 240, "framebuffer: 640x480 rgb565", WHITE, NAVY);
    text(48, 264, "UART continua ativa", WHITE, NAVY);
    true
}

fn framebuffer() -> Option<(usize, usize, u32, u32)> {
    unsafe {
        if !read_volatile(addr_of!(AVAILABLE)) {
            return None;
        }
        Some((
            read_volatile(addr_of!(BUFFER)) as usize,
            read_volatile(addr_of!(PITCH)) as usize,
            read_volatile(addr_of!(WIDTH)),
            read_volatile(addr_of!(HEIGHT)),
        ))
    }
}

fn dimensions() -> (i32, i32) {
    unsafe {
        (
            read_volatile(addr_of!(WIDTH)) as i32,
            read_volatile(addr_of!(HEIGHT)) as i32,
        )
    }
}

fn glyph(character: char) -> [u8; 7] {
    if character.is_ascii_lowercase() {
        return match character {
            'a' => [0, 0, 0b01110, 0b00001, 0b01111, 0b10001, 0b01111],
            'b' => [0b10000, 0b10000, 0b10110, 0b11001, 0b10001, 0b10001, 0b11110],
            'c' => [0, 0, 0b01111, 0b10000, 0b10000, 0b10000, 0b01111],
            'd' => [0b00001, 0b00001, 0b01101, 0b10011, 0b10001, 0b10001, 0b01111],
            'e' => [0, 0, 0b01110, 0b10001, 0b11111, 0b10000, 0b01111],
            'f' => [0b00110, 0b01001, 0b01000, 0b11100, 0b01000, 0b01000, 0b01000],
            'g' => [0, 0, 0b01111, 0b10001, 0b01111, 0b00001, 0b01110],
            'h' => [0b10000, 0b10000, 0b10110, 0b11001, 0b10001, 0b10001, 0b10001],
            'i' => [0b00100, 0, 0b01100, 0b00100, 0b00100, 0b00100, 0b01110],
            'j' => [0b00010, 0, 0b00110, 0b00010, 0b00010, 0b10010, 0b01100],
            'k' => [0b10000, 0b10000, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010],
            'l' => [0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
            'm' => [0, 0, 0b11010, 0b10101, 0b10101, 0b10101, 0b10101],
            'n' => [0, 0, 0b10110, 0b11001, 0b10001, 0b10001, 0b10001],
            'o' => [0, 0, 0b01110, 0b10001, 0b10001, 0b10001, 0b01110],
            'p' => [0, 0, 0b11110, 0b10001, 0b11110, 0b10000, 0b10000],
            'q' => [0, 0, 0b01111, 0b10001, 0b01111, 0b00001, 0b00001],
            'r' => [0, 0, 0b10111, 0b11000, 0b10000, 0b10000, 0b10000],
            's' => [0, 0, 0b01111, 0b10000, 0b01110, 0b00001, 0b11110],
            't' => [0b01000, 0b01000, 0b11100, 0b01000, 0b01000, 0b01001, 0b00110],
            'u' => [0, 0, 0b10001, 0b10001, 0b10001, 0b10011, 0b01101],
            'v' => [0, 0, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100],
            'w' => [0, 0, 0b10001, 0b10101, 0b10101, 0b10101, 0b01010],
            'x' => [0, 0, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001],
            'y' => [0, 0, 0b10001, 0b10001, 0b01111, 0b00001, 0b01110],
            'z' => [0, 0, 0b11111, 0b00010, 0b00100, 0b01000, 0b11111],
            _ => unreachable!(),
        };
    }
    let uppercase = character.to_ascii_uppercase();
    match uppercase {
        'A' => [
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'B' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ],
        'C' => [
            0b01111, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b01111,
        ],
        'D' => [
            0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
        ],
        'E' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
        'F' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'G' => [
            0b01111, 0b10000, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111,
        ],
        'H' => [
            0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'I' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b11111,
        ],
        'J' => [
            0b00111, 0b00010, 0b00010, 0b00010, 0b10010, 0b10010, 0b01100,
        ],
        'K' => [
            0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
        ],
        'L' => [
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ],
        'M' => [
            0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
        ],
        'N' => [
            0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001,
        ],
        'O' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'P' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'Q' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101,
        ],
        'R' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ],
        'S' => [
            0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        'T' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'U' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'V' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
        ],
        'W' => [
            0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001,
        ],
        'X' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
        ],
        'Y' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'Z' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
        ],
        '0' => [
            0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
        '1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        '2' => [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
        ],
        '3' => [
            0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        '4' => [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
        '5' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b00001, 0b00001, 0b11110,
        ],
        '6' => [
            0b01110, 0b10000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
        '7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        '8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        '9' => [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b01110,
        ],
        ' ' => [0; 7],
        ':' => [
            0b00000, 0b00100, 0b00100, 0b00000, 0b00100, 0b00100, 0b00000,
        ],
        '-' => [
            0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000,
        ],
        '/' => [
            0b00001, 0b00010, 0b00010, 0b00100, 0b01000, 0b01000, 0b10000,
        ],
        '.' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00110, 0b00110,
        ],
        _ => [
            0b11111, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11111,
        ],
    }
}

fn negotiate() -> Option<(u64, usize, u32, u32, u32)> {
    unsafe {
        let words = addr_of_mut!(MESSAGE.words) as *mut u32;
        for index in 0..35 {
            write_volatile(words.add(index), 0);
        }
        write_volatile(words.add(0), 35 * 4);
        write_volatile(words.add(2), TAG_PHYSICAL_SIZE);
        write_volatile(words.add(3), 8);
        write_volatile(words.add(4), 8);
        write_volatile(words.add(5), REQUESTED_WIDTH);
        write_volatile(words.add(6), REQUESTED_HEIGHT);
        write_volatile(words.add(7), TAG_VIRTUAL_SIZE);
        write_volatile(words.add(8), 8);
        write_volatile(words.add(9), 8);
        write_volatile(words.add(10), REQUESTED_WIDTH);
        write_volatile(words.add(11), REQUESTED_HEIGHT);
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
        if width != REQUESTED_WIDTH
            || height != REQUESTED_HEIGHT
            || virtual_width != REQUESTED_WIDTH
            || virtual_height != REQUESTED_HEIGHT
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
        Some((buffer_start, buffer_size, pitch, width, height))
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
