//! Programa em EL0 e o retorno ao kernel quando ele toca memória privilegiada.
//!
//! A janela virtual fica em `0x3C200000`: código, dados e pilha, cada um na
//! sua página. O texto do kernel em `0x80000` continua com permissão só de
//! EL1. A leitura de lá gera abort de dados vindo do EL0; o vetor devolve o
//! núcleo ao comando, com o console ainda vivo.

use core::arch::global_asm;
use core::ptr::{addr_of, addr_of_mut};
use core::sync::atomic::{AtomicU64, Ordering};

use crate::mmu;
use crate::uart;

global_asm!(include_str!("user.s"));

unsafe extern "C" {
    static user_blob: u8;
    static user_blob_end: u8;
    fn user_enter(entry: u64, stack: u64, data: u64, probe: u64);
    fn user_kernel_return();
}

/// EL1h com D, A e F mascarados e a IRQ aberta, como o console deixa o núcleo.
pub const RETURN_SPSR: u64 = 0x345;

const FLAG_OFF: usize = 8;

static FAULT_ESR: AtomicU64 = AtomicU64::new(0);
static FAULT_FAR: AtomicU64 = AtomicU64::new(0);
static FAULT_SPSR: AtomicU64 = AtomicU64::new(0);

#[repr(C, align(4096))]
struct Page {
    bytes: [u8; 4096],
}

static mut CODE: Page = Page { bytes: [0; 4096] };
static mut DATA: Page = Page { bytes: [0; 4096] };
static mut STACK: Page = Page { bytes: [0; 4096] };

/// `SPSR` de EL0t: os bits de modo [4:0] são zero.
pub fn from_el0(spsr: u64) -> bool {
    spsr & 0x1f == 0
}

pub fn kernel_return() -> u64 {
    user_kernel_return as *const () as u64
}

/// Guarda o abort que o vetor viu, para o comando imprimir depois do retorno.
pub fn note(esr: u64, far: u64, spsr: u64) {
    FAULT_ESR.store(esr, Ordering::Relaxed);
    FAULT_FAR.store(far, Ordering::Relaxed);
    FAULT_SPSR.store(spsr, Ordering::Relaxed);
}

/// Copia `blob` para a página de código e entra em EL0. Volta quando o
/// programa falta ou encerra pela chamada de saída.
pub fn launch(blob: *const u8, len: usize, probe: u64) -> bool {
    let code = addr_of_mut!(CODE);
    let data = addr_of_mut!(DATA);
    let stack = addr_of_mut!(STACK);
    if !mmu::map_user(code as u64, data as u64, stack as u64) {
        return false;
    }
    publish(code, data, blob, len);
    unsafe {
        user_enter(
            mmu::USER_CODE_VA,
            mmu::USER_STACK_TOP,
            mmu::USER_DATA_VA,
            probe,
        );
    }
    true
}

pub fn data_ptr() -> *mut u8 {
    addr_of_mut!(DATA) as *mut u8
}

pub fn exercise() {
    let blob = addr_of!(user_blob);
    let len = addr_of!(user_blob_end) as usize - blob as usize;
    uart::puts("usuario: el0\r\n");
    if !launch(blob, len, mmu::KERNEL_PROBE) {
        uart::puts("usuario: sem janela\r\n");
        return;
    }
    let flag = unsafe { core::ptr::read_volatile(data_ptr().add(FLAG_OFF) as *const u32) };
    let proprio = if flag == 1 { "ok" } else { "falhou" };
    let esr = FAULT_ESR.load(Ordering::Relaxed);
    let far = FAULT_FAR.load(Ordering::Relaxed);
    let spsr = FAULT_SPSR.load(Ordering::Relaxed);
    let modo = if from_el0(spsr) { "EL0" } else { "outro" };
    let class = (esr >> 26) & 0x3f;
    uart::write_fmt(format_args!("proprio: {proprio}\r\nmodo: {modo}\r\n"));
    if class == 0x24 && far == mmu::KERNEL_PROBE {
        uart::puts("usuario: kernel negado\r\n");
    } else {
        uart::write_fmt(format_args!(
            "usuario: inesperado esr {esr:#x} far {far:#x}\r\n"
        ));
    }
}

fn publish(code: *mut Page, data: *mut Page, blob: *const u8, len: usize) {
    unsafe {
        core::ptr::write_bytes(data as *mut u8, 0, 4096);
        core::ptr::write_bytes(code as *mut u8, 0, 4096);
        if len > 0 && len <= 4096 {
            core::ptr::copy_nonoverlapping(blob, code as *mut u8, len);
        }
        let mut line = code as usize;
        let end = line + 4096;
        while line < end {
            core::arch::asm!("dc civac, {line}", line = in(reg) line, options(nostack));
            line += 64;
        }
        core::arch::asm!("dsb sy", "ic iallu", "dsb sy", "isb", options(nostack));
    }
}
