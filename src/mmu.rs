//! MMU de estágio 1, identidade, grânulo de 4 KiB.
//!
//! No EL1, no EL2 e no EL3 o `TCR` usa `T0SZ = 32` e `TG0 = 4 KiB`, então a
//! caminhada começa no nível 1. O nível 2 cobre o primeiro 1 GiB com blocos
//! de 2 MiB, menos os primeiros 2 MiB, que descem a páginas de 4 KiB para
//! separar o código executável dos dados graváveis.
//!
//! A RAM do mailbox (`0..0x3C000000`) fica como memória normal. O buraco até
//! `0x3F000000` não tem descritor: o acesso a `0x3E000000` gera abort de
//! dados. O MMIO `0x3F000000..0x40000000` é device-nGnRnE.
//!
//! O EL0 usa as mesmas tabelas. As páginas do kernel ficam com `AP[2:1]`
//! privilegiado, então o EL0 não as lê. A janela do programa de usuário é
//! outro espaço: três páginas em `0x3C200000`, fora da RAM e do buraco do
//! teste `pagina`, apontando para molduras físicas do kernel.

use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};

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
const AP_EL0: u64 = 1 << 6;
const PXN: u64 = 1 << 53;
const XN: u64 = 1 << 54;

/// Janela do EL0, no vão entre a RAM e o MMIO. Não é o buraco de `pagina`.
pub const USER_CODE_VA: u64 = 0x3C20_0000;
pub const USER_DATA_VA: u64 = USER_CODE_VA + PAGE;
pub const USER_STACK_TOP: u64 = USER_CODE_VA + 3 * PAGE;
/// Primeira página do texto do kernel. O EL0 não tem permissão sobre ela.
pub const KERNEL_PROBE: u64 = 0x8_0000;
const ATTR_NORMAL: u64 = 0 << 2;
const ATTR_DEVICE: u64 = 1 << 2;

const L2_MMIO: usize = (MMIO_BASE / BLOCK) as usize;
const L2_RAM_END: usize = (RAM_END / BLOCK) as usize;

#[derive(Copy, Clone)]
#[repr(C, align(4096))]
struct Table {
    entries: [u64; 512],
}

static mut L1: Table = Table { entries: [0; 512] };
static mut L2: Table = Table { entries: [0; 512] };
static mut L3: Table = Table { entries: [0; 512] };
static mut L3_USER: Table = Table { entries: [0; 512] };
static mut L3_FRAMEBUFFER: [Table; 504] = [Table { entries: [0; 512] }; 504];
static mut CURRENT_EL: u8 = 0;

unsafe extern "C" {
    static __text_start: u8;
    static __text_end: u8;
    static __rodata_start: u8;
    static __rodata_end: u8;
    static __bss_start: u8;
    static __stacks_end: u8;
    static mut MMU_READY: u8;
}

const USER_L2: usize = (USER_CODE_VA / BLOCK) as usize;

const _: () = {
    assert!((HOLE >> 21) & 0x1ff == 496);
    assert!((MMIO_BASE >> 21) & 0x1ff == 504);
    assert!((RAM_END >> 21) & 0x1ff == 480);
    assert!(USER_L2 == 481);
    assert!(USER_L2 != (HOLE >> 21) as usize);
};

pub fn enable(el: u8) {
    map();
    unsafe { core::ptr::write_volatile(addr_of_mut!(CURRENT_EL), el) };
    unsafe {
        core::arch::asm!("dsb sy", "isb", options(nostack, preserves_flags));
    }
    publish_ready();
    install(el);
}

/// Liga a mesma tradução num núcleo que acabou de sair do spin table.
/// As tabelas já foram gravadas pelo núcleo 0.
pub fn enable_secondary(el: u8) {
    install(el);
}

/// Acorda os núcleos que entraram em `_start` antes das tabelas existirem.
fn publish_ready() {
    unsafe {
        let flag = addr_of_mut!(MMU_READY);
        write_volatile(flag, 1);
        let addr = flag as usize;
        core::arch::asm!(
            "dc civac, {addr}",
            "dsb sy",
            "sev",
            addr = in(reg) addr,
            options(nostack),
        );
    }
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

/// Publica a janela do EL0. Devolve falso se o bloco de 2 MiB já era RAM.
///
/// Código só de leitura e executável no EL0. Dados e pilha graváveis e não
/// executáveis. `PXN` impede o EL1 de executar essas páginas.
/// O EL0 pode pedir leitura de qualquer byte da janela dele.
pub fn user_readable(addr: u64, len: u64) -> bool {
    covered(addr, len, USER_CODE_VA, USER_STACK_TOP - USER_CODE_VA)
}

/// O kernel só escreve nas páginas de dados e de pilha. O código é só leitura.
pub fn user_writable(addr: u64, len: u64) -> bool {
    covered(addr, len, USER_DATA_VA, PAGE) || covered(addr, len, USER_STACK_TOP - PAGE, PAGE)
}

fn covered(addr: u64, len: u64, base: u64, size: u64) -> bool {
    let Some(end) = addr.checked_add(len) else {
        return false;
    };
    addr >= base && end <= base + size
}

pub fn map_user(code_pa: u64, data_pa: u64, stack_pa: u64) -> bool {
    unsafe {
        let l2 = addr_of_mut!(L2.entries[USER_L2]);
        let expect = table(addr_of!(L3_USER) as u64);
        let current = read_volatile(l2);
        if current != 0 && current != expect {
            return false;
        }
        let l3 = addr_of_mut!(L3_USER.entries[0]);
        for index in 0..512 {
            write_volatile(l3.add(index), 0);
        }
        write_volatile(l3, user_page(code_pa, false));
        write_volatile(l3.add(1), user_page(data_pa, true));
        write_volatile(l3.add(2), user_page(stack_pa, true));
        write_volatile(l2, expect);
        core::arch::asm!(
            "dsb sy",
            "tlbi vmalle1",
            "dsb sy",
            "isb",
            options(nostack),
        );
    }
    true
}

/// Mapeia o intervalo físico do framebuffer na mesma janela de identidade.
///
/// O intervalo recebido já deve ser um endereço físico ARM, não o endereço
/// de barramento devolvido pelo VideoCore. As páginas são normais,
/// compartilháveis internamente, graváveis, não executáveis e acessíveis
/// somente ao EL privilegiado.
pub fn map_framebuffer(start: u64, size: usize) -> bool {
    let Some(end) = start.checked_add(size as u64) else {
        return false;
    };
    if start < 0x0008_0000 || end > 0x3F00_0000 || end <= start {
        return false;
    }

    let map_start = start & !(PAGE - 1);
    let Some(map_end) = end.checked_add(PAGE - 1).map(|value| value & !(PAGE - 1)) else {
        return false;
    };
    if map_start < USER_STACK_TOP && map_end > USER_CODE_VA {
        return false;
    }

    let first_l2 = (map_start / BLOCK) as usize;
    let last_l2 = ((map_end - 1) / BLOCK) as usize;
    if last_l2 >= 504 {
        return false;
    }

    unsafe {
        for l2_index in first_l2..=last_l2 {
            let l3 = addr_of_mut!(L3_FRAMEBUFFER[l2_index]);
            let l2_entry = read_volatile(addr_of!(L2.entries[l2_index]));
            let old_table = if l2_index == 0 {
                table(addr_of!(L3) as u64)
            } else {
                0
            };

            for page in 0..512usize {
                let pa = (l2_index as u64) * BLOCK + (page as u64) * PAGE;
                let entry = if l2_entry == old_table {
                    read_volatile(addr_of!(L3.entries[page]))
                } else if l2_entry & 0b11 == VALID_BLOCK {
                    block_page_desc(pa, l2_index >= L2_MMIO)
                } else {
                    0
                };
                write_volatile(addr_of_mut!((*l3).entries[page]), entry);
            }

            let block_start = (l2_index as u64) * BLOCK;
            let block_end = block_start + BLOCK;
            let from = map_start.max(block_start);
            let to = map_end.min(block_end);
            let first_page = ((from - block_start) / PAGE) as usize;
            let last_page = ((to - block_start) / PAGE) as usize;
            for page in first_page..last_page {
                let pa = block_start + (page as u64) * PAGE;
                write_volatile(
                    addr_of_mut!((*l3).entries[page]),
                    framebuffer_page_desc(pa),
                );
            }
            write_volatile(
                addr_of_mut!(L2.entries[l2_index]),
                table_desc(l3 as u64),
            );
        }

        core::arch::asm!("dsb sy", options(nostack, preserves_flags));
        match read_volatile(addr_of!(CURRENT_EL)) {
            1 => core::arch::asm!("tlbi vmalle1", options(nostack)),
            2 => core::arch::asm!("tlbi alle2", options(nostack)),
            3 => core::arch::asm!("tlbi alle3", options(nostack)),
            _ => return false,
        }
        core::arch::asm!("dsb sy", "isb", options(nostack, preserves_flags));
    }
    true
}

fn table_desc(pa: u64) -> u64 {
    (pa & !(PAGE - 1)) | VALID_PAGE
}

fn block_page_desc(pa: u64, device: bool) -> u64 {
    let mut desc = (pa & !(PAGE - 1)) | VALID_PAGE | AF | SH_INNER | ATTR_NORMAL | XN;
    if device {
        desc = (pa & !(PAGE - 1)) | VALID_PAGE | AF | SH_OUTER | ATTR_DEVICE | XN;
    }
    desc
}

fn framebuffer_page_desc(pa: u64) -> u64 {
    (pa & !(PAGE - 1)) | VALID_PAGE | AF | SH_INNER | ATTR_NORMAL | XN
}

/// `AP[2:1] = 01` grava no EL0. `AP[2:1] = 11` só lê. Bit 54 é `UXN`.
fn user_page(pa: u64, writable: bool) -> u64 {
    let mut desc = (pa & !0xfff) | VALID_PAGE | AF | SH_INNER | ATTR_NORMAL | AP_EL0 | PXN | XN;
    if !writable {
        desc |= AP_RO;
        desc &= !XN;
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
