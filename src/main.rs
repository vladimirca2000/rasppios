#![no_std]
#![no_main]

mod console;
mod exception;
mod fs;
mod load;
mod mmu;
mod panic;
mod physmem;
mod sched;
mod sd;
mod sync;
mod syscall;
mod timer;
mod uart;
mod usb;
mod user;
mod video;

use core::arch::global_asm;

global_asm!(include_str!("boot.s"));

unsafe extern "C" {
    static CORE_ARRIVED: [u8; 4];
}

#[no_mangle]
pub extern "C" fn rust_main() -> ! {
    uart::init();
    exception::install();
    physmem::init();

    let el = exception_level();
    let id = core_id();
    let arrived = arrived_flags();
    mmu::enable(el);
    if !video::init() {
        uart::puts("video: indisponivel\r\n");
    } else if !video::exercise() {
        uart::puts("video: desenho falhou\r\n");
    }
    mmu::publish_secondary();
    timer::start(el);

    uart::write_fmt(format_args!(
        "rasppios: marco 17\r\n\
         kernel AArch64 carregado em 0x80000\r\n\
         exception level: EL{el}\r\n\
         nucleo em execucao: {id}\r\n\
         chegada: 0={a0} 1={a1} 2={a2} 3={a3}\r\n\
         timer: 100 Hz\r\n\
         mmu: ligada\r\n",
        a0 = flag(arrived[0]),
        a1 = flag(arrived[1]),
        a2 = flag(arrived[2]),
        a3 = flag(arrived[3]),
    ));
    if id == 0 {
        uart::puts("kernel somente no nucleo 0\r\n");
    } else {
        uart::puts("ERRO nucleo inesperado\r\n");
    }
    usb::init();

    console::run(el, id, arrived);
}

/// Núcleos 1–3 chegam aqui depois do spin table, com a pilha já armada.
#[no_mangle]
pub extern "C" fn rust_secondary() -> ! {
    let el = exception_level();
    mmu::enable_secondary(el);
    exception::install();
    sync::worker();
}

/// `CORE_ARRIVED` é escrito em `boot.s` com um byte por núcleo.
fn arrived_flags() -> [u8; 4] {
    let mut arrived = [0u8; 4];
    for core in 0..4 {
        arrived[core] = unsafe { core::ptr::read_volatile(CORE_ARRIVED.as_ptr().add(core)) };
    }
    arrived
}

fn flag(seen: u8) -> u8 {
    if seen == 0 {
        0
    } else {
        1
    }
}

/// Aff0 de `MPIDR_EL1`: 0..3 nos quatro Cortex-A53.
fn core_id() -> u8 {
    let id: u64;
    unsafe {
        core::arch::asm!("mrs {}, mpidr_el1", out(reg) id, options(nomem, nostack));
    }
    (id & 0xff) as u8
}

/// `CurrentEL` guarda o nível nos bits [3:2].
fn exception_level() -> u8 {
    let el: u64;
    unsafe {
        core::arch::asm!("mrs {}, CurrentEL", out(reg) el, options(nomem, nostack));
    }
    ((el >> 2) & 0b11) as u8
}
