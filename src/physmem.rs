//! Mapa da RAM e alocador físico por encaixe.
//!
//! O QEMU 8.2.2 (`hw/misc/bcm2835_mbox.c`) coloca o mailbox em
//! `0x3F000000 + 0xB800`. A leitura do ARM é o offset `0x80`, o status
//! `0x98` e a escrita para o VideoCore o offset `0xA0`. Bit 31 cheio,
//! bit 30 vazio. O canal 8 é o de propriedades
//! (`include/hw/arm/raspberrypi-fw-defs.h`, tag `0x00010005`).
//!
//! A resposta do `raspi3b` é base 0 e tamanho `0x3C000000`: 1 GiB de RAM
//! menos 64 MiB de VideoCore (`DEFAULT_VCRAM_SIZE` em `bcm2835_fb.c`).
//! O heap começa depois das pilhas e para antes dessa reserva e do
//! MMIO em `0x3F000000`.

use core::ptr::{addr_of, read_volatile, write_volatile};

use crate::uart;

const PERIPH_BASE: usize = 0x3F00_0000;
const MBOX_BASE: usize = PERIPH_BASE + 0xB800;
const MBOX_READ: usize = MBOX_BASE + 0x80;
const MBOX_STATUS: usize = MBOX_BASE + 0x98;
const MBOX_WRITE: usize = MBOX_BASE + 0xA0;

const MBOX_FULL: u32 = 1 << 31;
const MBOX_EMPTY: u32 = 1 << 30;
const MBOX_CHANNEL: u32 = 8;
const TAG_ARM_MEMORY: u32 = 0x0001_0005;
const BUS_ALIAS: usize = 0xC000_0000;

const MMIO_BASE: usize = 0x3F00_0000;
const MMIO_END: usize = 0x4000_0000;

/// Reserva usada se o mailbox não responder. Igual ao padrão do QEMU 8.2.
const FALLBACK_ARM_END: usize = 0x3C00_0000;

const HDR: usize = 16;
const ALIGN: usize = 16;

struct Free {
    size: usize,
    next: *mut Free,
}

unsafe extern "C" {
    static __stacks_end: u8;
}

#[repr(C, align(16))]
struct Message {
    words: [u32; 8],
}

static mut MESSAGE: Message = Message { words: [0; 8] };
static mut HEAD: *mut Free = core::ptr::null_mut();
static mut ARM_BASE: usize = 0;
static mut ARM_END: usize = 0;
static mut FROM_MAILBOX: bool = false;
static mut HEAP_START: usize = 0;
static mut HEAP_END: usize = 0;

/// Só o núcleo 0 chama estas funções, e a IRQ do timer não mexe na lista.
pub fn init() {
    let (base, end, from_mailbox) = match arm_memory() {
        Some((base, size)) => (base, base + size, true),
        None => (0, FALLBACK_ARM_END, false),
    };
    let end = end.min(MMIO_BASE);
    let start = align_up(stacks_end(), ALIGN);
    unsafe {
        ARM_BASE = base;
        ARM_END = end;
        FROM_MAILBOX = from_mailbox;
        HEAP_START = start;
        HEAP_END = end;
        HEAD = core::ptr::null_mut();
        if end > start + HDR + ALIGN {
            let block = start as *mut Free;
            core::ptr::write(
                block,
                Free {
                    size: end - start,
                    next: core::ptr::null_mut(),
                },
            );
            HEAD = block;
        }
    }
}

pub fn report() {
    let (arm_base, arm_end, mailbox, heap_start, heap_end) =
        unsafe { (ARM_BASE, ARM_END, FROM_MAILBOX, HEAP_START, HEAP_END) };
    let origem = if mailbox { "mailbox" } else { "reserva" };
    uart::write_fmt(format_args!(
        "ram: 0x{arm_base:08x} 0x{size:08x} {origem}\r\n\
         mmio: 0x{MMIO_BASE:08x} 0x{MMIO_END:08x}\r\n\
         heap: 0x{heap_start:08x} 0x{heap_end:08x}\r\n",
        size = arm_end - arm_base,
    ));

    let first = alloc(4096);
    let second = alloc(4096);
    match (first, second) {
        (Some(a), Some(b)) => {
            uart::write_fmt(format_args!("bloco: 0x{a:08x} 4096\r\n"));
            uart::write_fmt(format_args!("bloco: 0x{b:08x} 4096\r\n"));
            let overlap = ranges_overlap(a, b, 4096);
            uart::puts(if overlap {
                "sobrepostos: sim\r\n"
            } else {
                "sobrepostos: nao\r\n"
            });
            free(a);
            match alloc(4096) {
                Some(again) => {
                    uart::puts("liberar: ok\r\n");
                    uart::puts(if again == a {
                        "reuso: ok\r\n"
                    } else {
                        "reuso: falhou\r\n"
                    });
                    free(again);
                }
                None => uart::puts("liberar: falhou\r\n"),
            }
            free(b);
        }
        _ => uart::puts("sobrepostos: falhou\r\n"),
    }
}

fn alloc(bytes: usize) -> Option<usize> {
    if bytes == 0 {
        return None;
    }
    let need = align_up(bytes + HDR, ALIGN);
    unsafe {
        let mut prev: *mut Free = core::ptr::null_mut();
        let mut cur = HEAD;
        while !cur.is_null() {
            let size = (*cur).size;
            let next = (*cur).next;
            if size >= need {
                let remain = size - need;
                if remain >= HDR + ALIGN {
                    let rest = (cur as usize + need) as *mut Free;
                    core::ptr::write(rest, Free { size: remain, next });
                    (*cur).size = need;
                    link(prev, rest);
                } else {
                    link(prev, next);
                }
                return Some(cur as usize + HDR);
            }
            prev = cur;
            cur = next;
        }
    }
    None
}

fn free(user: usize) {
    let block = (user - HDR) as *mut Free;
    unsafe {
        (*block).next = HEAD;
        HEAD = block;
    }
}

fn link(prev: *mut Free, next: *mut Free) {
    unsafe {
        if prev.is_null() {
            HEAD = next;
        } else {
            (*prev).next = next;
        }
    }
}

fn ranges_overlap(a: usize, b: usize, len: usize) -> bool {
    let (lo, hi) = if a < b { (a, b) } else { (b, a) };
    lo + len > hi
}

fn stacks_end() -> usize {
    addr_of!(__stacks_end) as usize
}

fn align_up(value: usize, align: usize) -> usize {
    (value + align - 1) & !(align - 1)
}

fn arm_memory() -> Option<(usize, usize)> {
    let phys = addr_of!(MESSAGE) as usize;
    unsafe {
        let base = addr_of!(MESSAGE.words) as *mut u32;
        let values = [32u32, 0, TAG_ARM_MEMORY, 8, 0, 0, 0, 0];
        for (index, value) in values.into_iter().enumerate() {
            write_volatile(base.add(index), value);
        }
    }
    dsb();

    if !wait(|status| status & MBOX_FULL == 0) {
        return None;
    }
    let bus = ((phys | BUS_ALIAS) as u32) & !0xf;
    unsafe { write_volatile(MBOX_WRITE as *mut u32, bus | MBOX_CHANNEL) };
    dsb();

    if !wait(|status| status & MBOX_EMPTY == 0) {
        return None;
    }
    let reply = unsafe { read_volatile(MBOX_READ as *const u32) };
    if reply & 0xf != MBOX_CHANNEL {
        return None;
    }
    dsb();

    let words = unsafe {
        [
            read_volatile(addr_of!(MESSAGE.words[1])),
            read_volatile(addr_of!(MESSAGE.words[4])),
            read_volatile(addr_of!(MESSAGE.words[5])),
            read_volatile(addr_of!(MESSAGE.words[6])),
        ]
    };
    let code = words[0];
    let tag_code = words[1];
    let base = words[2] as usize;
    let size = words[3] as usize;
    if code & (1 << 31) == 0 || tag_code & (1 << 31) == 0 {
        return None;
    }
    if base != 0 || size < 0x1000_0000 || size > MMIO_BASE || size % 4096 != 0 {
        return None;
    }
    Some((base, size))
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
