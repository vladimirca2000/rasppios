//! Trava de exclusão e o teste entre os quatro núcleos.
//!
//! O spin table do Pi 3 e do QEMU 8.2 fica em `0xd8`, `0xe0`, `0xe8` e `0xf0`.
//! O núcleo 0 grava `_start`, limpa a linha até o ponto de coerência e manda
//! `sev`. Cada núcleo soma 1000 numa variável comum, com a carga e o armazena-
//! mento separados, para que uma trava furada perca atualizações.

use core::ptr::{addr_of, write_volatile};
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use crate::mmu;
use crate::uart;

const PER_CORE: u64 = 1000;
const CORES: u32 = 4;
const SPIN_TABLE: usize = 0xd8;

static LOCK: AtomicBool = AtomicBool::new(false);
static READY: AtomicU32 = AtomicU32::new(0);
static DONE: AtomicU32 = AtomicU32::new(0);
static JOB: AtomicU32 = AtomicU32::new(0);
static mut COUNTER: u64 = 0;

unsafe extern "C" {
    fn _start();
}

pub fn exercise() {
    wake();
    if !wait_ready() {
        uart::write_fmt(format_args!(
            "trava: nucleos {} incompletos\r\n",
            READY.load(Ordering::Acquire) + 1
        ));
        return;
    }
    unsafe { write_volatile(addr_of!(COUNTER).cast_mut(), 0) };
    DONE.store(0, Ordering::Relaxed);
    JOB.store(1, Ordering::Release);
    sev();
    run_share();
    while DONE.load(Ordering::Acquire) < CORES {
        core::hint::spin_loop();
    }
    let sum = unsafe { core::ptr::read_volatile(addr_of!(COUNTER)) };
    JOB.store(0, Ordering::Release);
    sev();
    uart::write_fmt(format_args!("trava: nucleos {CORES} soma {sum}\r\n"));
}

pub fn worker() -> ! {
    READY.fetch_add(1, Ordering::Release);
    loop {
        while JOB.load(Ordering::Acquire) == 0 {
            wfe();
        }
        run_share();
        while JOB.load(Ordering::Acquire) != 0 {
            core::hint::spin_loop();
        }
    }
}

fn run_share() {
    for _ in 0..PER_CORE {
        lock();
        unsafe {
            let value = core::ptr::read_volatile(addr_of!(COUNTER));
            write_volatile(addr_of!(COUNTER).cast_mut(), value + 1);
        }
        unlock();
    }
    DONE.fetch_add(1, Ordering::Release);
}

fn lock() {
    while LOCK
        .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        while LOCK.load(Ordering::Relaxed) {
            core::hint::spin_loop();
        }
    }
}

fn unlock() {
    LOCK.store(false, Ordering::Release);
}

fn wake() {
    mmu::publish();
    for core in 1..CORES as usize {
        let slot = (SPIN_TABLE + core * 8) as *mut u64;
        unsafe {
            write_volatile(slot, _start as *const () as usize as u64);
            let addr = slot as usize;
            core::arch::asm!("dc civac, {addr}", addr = in(reg) addr, options(nostack));
        }
    }
    unsafe {
        core::arch::asm!("dsb sy", "sev", options(nostack, preserves_flags));
    }
}

fn wait_ready() -> bool {
    for _ in 0..50_000_000 {
        if READY.load(Ordering::Acquire) >= CORES - 1 {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

fn sev() {
    unsafe { core::arch::asm!("sev", options(nostack, preserves_flags)) };
}

fn wfe() {
    unsafe { core::arch::asm!("wfe", options(nostack, preserves_flags)) };
}
