//! MMU de estágio 1, identidade, grânulo de 4 KiB.
//!
//! No EL2 e no EL3 o `TCR` usa `T0SZ = 32` e `TG0 = 4 KiB`, então a caminhada
//! começa no nível 1. O nível 2 cobre o primeiro 1 GiB com blocos de 2 MiB,
//! menos os primeiros 2 MiB, que descem a páginas de 4 KiB para separar o
//! código executável dos dados graváveis.
//!
//! A RAM do mailbox (`0..0x3C000000`) fica como memória normal. O buraco até
//! `0x3F000000` não tem descritor: o acesso a `0x3E000000` gera abort de
//! dados. O MMIO `0x3F000000..0x40000000` é device-nGnRnE.

use core::ptr::{addr_of, addr_of_mut, write_volatile};

use crate::uart;

/// Endereço de propósito deixado sem tradução. O `ldr` em `load_hole` é uma
/// instrução de 4 bytes; o relato soma 4 ao `ELR` só quando o `FAR` é este.
pub const HOLE: u64 = 0x3E00_0000;

const MMIO_BASE: u64 = 0x3F00_0000;
const RAM_END: u64 = 0x3C00_0000;
const BLOCK: u64 = 1 << 21;
const PAGE: u64 = 1 << 12;

const VALID_BLOCK: u64 = 0b01;
const VALID_PAGE: u64 = 0b11;
const AF: u64 = 1 << 10;
const SH_INNER: u64 = 3 << 8;
const SH_OUTER: u64 = 2 << 8;
const AP_RO: u64 = 1 << 7;
const XN: u64 = 1 << 54;
const ATTR_NORMAL: u64 = 0 << 2;
const ATTR_DEVICE: u64 = 1 << 2;

const L2_MMIO: usize = (MMIO_BASE / BLOCK) as usize;
const L2_RAM_END: usize = (RAM_END / BLOCK) as usize;

#[repr(C, align(4096))]
struct Table {
    entries: [u64; 512],
}

static mut L1: Table = Table { entries: [0; 512] };
static mut L2: Table = Table { entries: [0; 512] };
static mut L3: Table = Table { entries: [0; 512] };

unsafe extern "C" {
    static __text_start: u8;
    static __text_end: u8;
    static __rodata_start: u8;
    static __rodata_end: u8;
    static __bss_start: u8;
    static __stacks_end: u8;
}

const _: () = {
    assert!((HOLE >> 21) & 0x1ff == 496);
    assert!((MMIO_BASE >> 21) & 0x1ff == 504);
    assert!((RAM_END >> 21) & 0x1ff == 480);
};

pub fn enable(el: u8) {
    map();
    unsafe {
        core::arch::asm!("dsb sy", "isb", options(nostack, preserves_flags));
    }
    install(el);
}

/// Liga a mesma tradução num núcleo que acabou de sair do spin table.
/// As tabelas já foram gravadas pelo núcleo 0.
pub fn enable_secondary(el: u8) {
    install(el);
}

/// Empurra para a RAM o que o núcleo 0 escreveu com o cache ligado.
/// O núcleo secundário ainda lê com a MMU desligada até copiar o `TTBR`.
pub fn publish() {
    let start = addr_of!(__bss_start) as usize & !63;
    let end = addr_of!(__stacks_end) as usize - 0x8000;
    let mut page = start;
    while page < end {
        unsafe {
            core::arch::asm!("dc civac, {page}", page = in(reg) page, options(nostack));
        }
        page += 64;
    }
    unsafe {
        core::arch::asm!("dsb sy", options(nostack, preserves_flags));
    }
}

fn install(el: u8) {
    let ttbr = addr_of!(L1) as u64;
    let mair: u64 = 0x00ff;
    let enable_bits: u64 = 1 | (1 << 2) | (1 << 12);
    unsafe {
        match el {
            1 => program_el1(mair, tcr_el1(), ttbr, enable_bits),
            2 => program_el2(mair, tcr_el2(), ttbr, enable_bits),
            3 => program_el3(mair, tcr_el2(), ttbr, enable_bits),
            _ => {}
        }
    }
}

pub fn exercise() {
    static mut CELL: u64 = 0;
    unsafe {
        write_volatile(addr_of_mut!(CELL), 0xA5A5_A5A5_A5A5_A5A5);
        let got = core::ptr::read_volatile(addr_of!(CELL));
        uart::write_fmt(format_args!("pagina: ok {got:#x}\r\n"));
        load_hole();
    }
    uart::puts("pagina: falha contida\r\n");
}

unsafe fn load_hole() {
    unsafe {
        core::arch::asm!(
            "ldr x0, [{addr}]",
            addr = in(reg) HOLE,
            out("x0") _,
            options(nostack),
        );
    }
}

fn map() {
    let text = span(addr_of!(__text_start), addr_of!(__text_end));
    let ro = span(addr_of!(__rodata_start), addr_of!(__rodata_end));
    unsafe {
        let l1 = addr_of_mut!(L1.entries[0]);
        let l2 = addr_of_mut!(L2.entries[0]);
        let l3 = addr_of_mut!(L3.entries[0]);
        write_volatile(l1, table(addr_of!(L2) as u64));
        write_volatile(l2, table(addr_of!(L3) as u64));

        for page in 0..512u64 {
            let pa = page * PAGE;
            let desc = if contains(text, pa) {
                page_desc(pa, false, false)
            } else if contains(ro, pa) {
                page_desc(pa, false, true)
            } else {
                page_desc(pa, true, true)
            };
            write_volatile(l3.add(page as usize), desc);
        }

        for index in 1..L2_RAM_END {
            let pa = (index as u64) * BLOCK;
            write_volatile(l2.add(index), block_desc(pa, false));
        }
        for index in L2_MMIO..512 {
            let pa = (index as u64) * BLOCK;
            write_volatile(l2.add(index), block_desc(pa, true));
        }
    }
}

fn span(start: *const u8, end: *const u8) -> (u64, u64) {
    (start as u64, end as u64)
}

fn contains(range: (u64, u64), pa: u64) -> bool {
    pa >= range.0 && pa + PAGE <= range.1
}

fn table(pa: u64) -> u64 {
    (pa & !0xfff) | VALID_PAGE
}

fn page_desc(pa: u64, writable: bool, xn: bool) -> u64 {
    let mut desc = (pa & !0xfff) | VALID_PAGE | AF | SH_INNER | ATTR_NORMAL;
    if !writable {
        desc |= AP_RO;
    }
    if xn {
        desc |= XN;
    }
    desc
}

fn block_desc(pa: u64, device: bool) -> u64 {
    let mut desc = (pa & !0x1f_ffff) | VALID_BLOCK | AF | XN;
    if device {
        desc |= ATTR_DEVICE | SH_OUTER;
    } else {
        desc |= ATTR_NORMAL | SH_INNER;
    }
    desc
}

/// `TCR_EL2` / `TCR_EL3` com `E2H` desligado: 32 bits de VA, cache WBWA, 4 KiB.
fn tcr_el2() -> u64 {
    32 | (0b01 << 8) | (0b01 << 10) | (0b11 << 12)
}

/// `TCR_EL1`: mesma metade baixa, e a metade alta desligada em `EPD1`.
fn tcr_el1() -> u64 {
    tcr_el2() | (32 << 16) | (1 << 23) | (0b10 << 30)
}

unsafe fn program_el1(mair: u64, tcr: u64, ttbr: u64, bits: u64) {
    unsafe {
        core::arch::asm!(
            "msr mair_el1, {mair}",
            "msr tcr_el1, {tcr}",
            "msr ttbr0_el1, {ttbr}",
            "isb",
            "ic iallu",
            "dsb sy",
            "tlbi vmalle1",
            "dsb sy",
            "isb",
            "mrs {sctlr}, sctlr_el1",
            "orr {sctlr}, {sctlr}, {bits}",
            "msr sctlr_el1, {sctlr}",
            "isb",
            mair = in(reg) mair,
            tcr = in(reg) tcr,
            ttbr = in(reg) ttbr,
            bits = in(reg) bits,
            sctlr = out(reg) _,
            options(nostack),
        );
    }
}

unsafe fn program_el2(mair: u64, tcr: u64, ttbr: u64, bits: u64) {
    unsafe {
        core::arch::asm!(
            "msr mair_el2, {mair}",
            "msr tcr_el2, {tcr}",
            "msr ttbr0_el2, {ttbr}",
            "isb",
            "ic iallu",
            "dsb sy",
            "tlbi alle2",
            "dsb sy",
            "isb",
            "mrs {sctlr}, sctlr_el2",
            "orr {sctlr}, {sctlr}, {bits}",
            "msr sctlr_el2, {sctlr}",
            "isb",
            mair = in(reg) mair,
            tcr = in(reg) tcr,
            ttbr = in(reg) ttbr,
            bits = in(reg) bits,
            sctlr = out(reg) _,
            options(nostack),
        );
    }
}

unsafe fn program_el3(mair: u64, tcr: u64, ttbr: u64, bits: u64) {
    unsafe {
        core::arch::asm!(
            "msr mair_el3, {mair}",
            "msr tcr_el3, {tcr}",
            "msr ttbr0_el3, {ttbr}",
            "isb",
            "ic iallu",
            "dsb sy",
            "tlbi alle3",
            "dsb sy",
            "isb",
            "mrs {sctlr}, sctlr_el3",
            "orr {sctlr}, {sctlr}, {bits}",
            "msr sctlr_el3, {sctlr}",
            "isb",
            mair = in(reg) mair,
            tcr = in(reg) tcr,
            ttbr = in(reg) ttbr,
            bits = in(reg) bits,
            sctlr = out(reg) _,
            options(nostack),
        );
    }
}
