//! Vetores AArch64 e o relato na miniUART.
//!
//! `ESR_ELx` classe 0x3C é a instrução `BRK` (ARM ARM, EC 0b111100).
//! O bit 25 (IL) fica ligado porque `BRK` tem 32 bits. Com imediato 1,
//! o syndrome esperado é `0xf2000001`.
//!
//! Classe 0x25 é abort de dados neste nível. Com IL e DFSC `0b000110`
//! (falha de tradução no nível 2) o syndrome do buraco é `0x96000006`.

use core::arch::global_asm;
use core::sync::atomic::{AtomicBool, Ordering};

use crate::mmu;
use crate::sched;
use crate::timer;
use crate::uart;

global_asm!(include_str!("vectors.s"));

unsafe extern "C" {
    static exception_vectors: u8;
}

const EC_SHIFT: u64 = 26;
const EC_BRK: u64 = 0x3c;
const EC_DATA_ABORT: u64 = 0x25;

/// Quadro montado por `vectors.s`. Os offsets são o contrato com o assembly.
#[repr(C)]
pub(crate) struct Frame {
    x: [u64; 31],
    elr: u64,
    spsr: u64,
    esr: u64,
    far: u64,
}

static IN_HANDLER: AtomicBool = AtomicBool::new(false);

const _: () = assert!(core::mem::size_of::<Frame>() == 280);

pub fn install() {
    let base = &raw const exception_vectors as u64;
    let el: u64;
    unsafe {
        core::arch::asm!("mrs {}, CurrentEL", out(reg) el, options(nomem, nostack));
        match (el >> 2) & 0b11 {
            1 => core::arch::asm!("msr vbar_el1, {base}", "isb", base = in(reg) base),
            2 => core::arch::asm!("msr vbar_el2, {base}", "isb", base = in(reg) base),
            3 => core::arch::asm!("msr vbar_el3, {base}", "isb", base = in(reg) base),
            _ => {}
        }
    }
}

/// `BRK #1` com `x0 = 0x41`, para o relato mostrar causa e registrador.
pub fn provoke_brk() {
    unsafe {
        core::arch::asm!(
            "mov x0, #0x41",
            "brk #1",
            out("x0") _,
            options(nostack),
        );
    }
}

/// Chamado pelo vetor com o quadro, o tipo e o SP de antes da entrada.
#[no_mangle]
pub extern "C" fn exception_entry(frame: *mut Frame, kind: u64, sp: u64) {
    if IN_HANDLER.swap(true, Ordering::SeqCst) {
        loop {
            core::hint::spin_loop();
        }
    }
    // O ponteiro é o quadro que `vectors.s` acabou de preencher nesta pilha.
    let frame = unsafe { &mut *frame };
    if kind == 1 && timer::handle_irq() {
        IN_HANDLER.store(false, Ordering::SeqCst);
        sched::on_tick();
        return;
    }
    report(frame, kind, sp);
    if kind == 0 && returns_to_caller(frame) {
        frame.elr = frame.elr.wrapping_add(4);
        IN_HANDLER.store(false, Ordering::SeqCst);
        return;
    }
    loop {
        core::hint::spin_loop();
    }
}

fn report(frame: &Frame, kind: u64, sp: u64) {
    uart::write_fmt(format_args!(
        "excecao: {}\r\n\
         causa: {}\r\n\
         esr: {:#010x}\r\n\
         elr: {:#018x}\r\n\
         spsr: {:#018x}\r\n\
         far: {:#018x}\r\n\
         sp: {:#018x}\r\n\
         x0: {:#018x}\r\n",
        kind_name(kind),
        cause(frame.esr),
        frame.esr,
        frame.elr,
        frame.spsr,
        frame.far,
        sp,
        frame.x[0],
    ));
}

fn returns_to_caller(frame: &Frame) -> bool {
    let class = ec(frame.esr);
    class == EC_BRK || (class == EC_DATA_ABORT && frame.far == mmu::HOLE)
}

fn ec(esr: u64) -> u64 {
    (esr >> EC_SHIFT) & 0x3f
}

fn kind_name(kind: u64) -> &'static str {
    match kind {
        0 => "sincrono",
        1 => "irq",
        2 => "fiq",
        3 => "serror",
        _ => "desconhecida",
    }
}

fn cause(esr: u64) -> &'static str {
    match ec(esr) {
        0x15 => "svc",
        0x16 => "hvc",
        0x17 => "smc",
        0x20 | 0x21 => "abort de instrucao",
        0x24 | 0x25 => "abort de dados",
        EC_BRK => "brk",
        _ => "desconhecida",
    }
}
