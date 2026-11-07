//! Chamadas do EL0. O número vai em `x8` e o imediato do `SVC` fica em zero.
//!
//! 0 escreve bytes da janela do usuário na miniUART. 1 lê da miniUART para
//! uma página gravável dessa janela. 2 devolve o `CLO` do system timer.
//! 3 encerra e devolve o status ao comando. Um ponteiro fora da janela
//! não é seguido: a chamada volta com `x0 = !0` e o EL0 continua.

use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{AtomicU64, Ordering};

use crate::mmu;
use crate::timer;
use crate::uart;
use crate::user;

const WRITE: u64 = 0;
const READ: u64 = 1;
const TIME: u64 = 2;
const EXIT: u64 = 3;
const BAD: u64 = u64::MAX;
const LIMIT: u64 = 256;

const TIME_OFF: usize = 16;
const BYTE_OFF: usize = 24;

static EXIT_CODE: AtomicU64 = AtomicU64::new(BAD);

pub enum Reply {
    Value(u64),
    Exit,
}

pub fn dispatch(number: u64, arg0: u64, arg1: u64) -> Reply {
    match number {
        WRITE => Reply::Value(write(arg0, arg1)),
        READ => Reply::Value(read(arg0, arg1)),
        TIME => Reply::Value(timer::clo() as u64),
        EXIT => {
            EXIT_CODE.store(arg0, Ordering::Relaxed);
            Reply::Exit
        }
        _ => Reply::Value(BAD),
    }
}

pub fn begin_exit() {
    EXIT_CODE.store(BAD, Ordering::Relaxed);
}

pub fn exit_status() -> u64 {
    EXIT_CODE.load(Ordering::Relaxed)
}

pub fn exercise() {
    unsafe extern "C" {
        static syscall_blob: u8;
        static syscall_blob_end: u8;
    }
    EXIT_CODE.store(BAD, Ordering::Relaxed);
    let blob = &raw const syscall_blob;
    let len = &raw const syscall_blob_end as usize - blob as usize;
    if !user::launch(blob, len, 0) {
        uart::puts("chamada: sem janela\r\n");
        return;
    }
    let data = user::data_ptr();
    // A página foi escrita pelo EL0 e lida aqui depois do retorno ao EL1.
    let denied = unsafe { read_volatile(data as *const u64) };
    let tempo = unsafe { read_volatile(data.add(TIME_OFF) as *const u64) };
    let byte = unsafe { read_volatile(data.add(BYTE_OFF)) };
    let ponteiro = if denied == BAD { "negado" } else { "aceitou" };
    let fim = EXIT_CODE.load(Ordering::Relaxed);
    uart::write_fmt(format_args!(
        "ponteiro: {ponteiro}\r\n\
         tempo: {tempo}\r\n\
         entrada: {}\r\n\
         fim: {fim}\r\n",
        byte as char
    ));
}

fn write(addr: u64, len: u64) -> u64 {
    if len > LIMIT || !mmu::user_readable(addr, len) {
        return BAD;
    }
    for index in 0..len {
        // O intervalo cabe na janela do EL0, mapeada também para o EL1.
        let byte = unsafe { read_volatile((addr as *const u8).add(index as usize)) };
        uart::putc(byte);
    }
    len
}

fn read(addr: u64, len: u64) -> u64 {
    if len > LIMIT || !mmu::user_writable(addr, len) {
        return BAD;
    }
    for index in 0..len {
        let byte = uart::getc();
        unsafe { write_volatile((addr as *mut u8).add(index as usize), byte) };
    }
    len
}
