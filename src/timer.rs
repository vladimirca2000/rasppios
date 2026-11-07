//! System timer do BCM2837 e a IRQ do canal 1.
//!
//! O contador livre (`CLO`) no QEMU 8.2 anda a 1 MHz. Um compare a cada
//! 10 000 contas produz 100 Hz. A IRQ entra pelo controlador em
//! `0x3F00_B200`, bit 1 de `Enable IRQs 1` e de `IRQ pending 1`.

use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{AtomicU64, Ordering};

const TIMER_CS: usize = 0x3F00_3000;
const TIMER_CLO: usize = 0x3F00_3004;
const TIMER_C1: usize = 0x3F00_3010;
const IRQ_PENDING1: usize = 0x3F00_B204;
const IRQ_ENABLE1: usize = 0x3F00_B210;

/// Canal 1. Os canais 0 e 2 ficam com o VideoCore no Pi.
const TIMER1: u32 = 1 << 1;
const CLOCK_HZ: u32 = 1_000_000;
const TICK_HZ: u32 = 100;
const PERIOD: u32 = CLOCK_HZ / TICK_HZ;

static TICKS: AtomicU64 = AtomicU64::new(0);

/// Endereços de MMIO do BCM2837, alinhados a 4.
///
/// # Safety
/// `addr` é um registrador deste módulo. O acesso é volatile.
unsafe fn wr(addr: usize, value: u32) {
    unsafe { write_volatile(addr as *mut u32, value) }
}

/// # Safety
/// `addr` é um registrador deste módulo.
unsafe fn rd(addr: usize) -> u32 {
    unsafe { read_volatile(addr as *const u32) }
}

pub fn start(el: u8) {
    unsafe {
        wr(TIMER_CS, TIMER1);
        let clo = rd(TIMER_CLO);
        wr(TIMER_C1, clo.wrapping_add(PERIOD));
        wr(IRQ_ENABLE1, TIMER1);
    }
    route(el);
    unsafe {
        core::arch::asm!("msr daifclr, #2", "isb");
    }
}

pub fn ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}

pub fn clo() -> u32 {
    unsafe { rd(TIMER_CLO) }
}

/// Trata a IRQ do canal 1. Devolve falso se a pendência é outra.
pub fn handle_irq() -> bool {
    unsafe {
        if (rd(IRQ_PENDING1) & TIMER1) == 0 {
            return false;
        }
        wr(TIMER_CS, TIMER1);
        let clo = rd(TIMER_CLO);
        wr(TIMER_C1, clo.wrapping_add(PERIOD));
    }
    TICKS.fetch_add(1, Ordering::Relaxed);
    true
}

fn route(el: u8) {
    unsafe {
        match el {
            2 => core::arch::asm!(
                "mrs {hcr}, hcr_el2",
                "orr {hcr}, {hcr}, #16",
                "msr hcr_el2, {hcr}",
                "isb",
                hcr = out(reg) _,
            ),
            3 => core::arch::asm!(
                "mrs {scr}, scr_el3",
                "orr {scr}, {scr}, #2",
                "msr scr_el3, {scr}",
                "isb",
                scr = out(reg) _,
            ),
            _ => {}
        }
    }
}
