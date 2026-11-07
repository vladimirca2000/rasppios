//! Carregador de programas em EL0.
//!
//! O arquivo `prog` no `RPFS` é a imagem. Os 16 primeiros bytes são o
//! cabeçalho `RPEX`, versão 1, deslocamento de entrada e tamanho do código.
//! O código é copiado para a janela `0x3C200000` e entra em EL0. A aplicação
//! escreve pela chamada 0 e termina pela chamada 3. O status volta em `x0`.

use core::arch::global_asm;

use crate::fs;
use crate::syscall;
use crate::uart;
use crate::user;

global_asm!(
    r#"
    .section .rodata
    .global app_code
    .global app_code_end
    .balign 4
    app_code:
        adr     x0, app_msg
        mov     x1, #9
        mov     x8, #0
        svc     #0
        mov     x0, #7
        mov     x8, #3
        svc     #0
    app_msg:
        .ascii  "app: ok\r\n"
    app_code_end:
    "#
);

unsafe extern "C" {
    static app_code: u8;
    static app_code_end: u8;
}

pub fn exercise() {
    let mut image = [0u8; 512];
    let Some(size) = build(&mut image) else {
        uart::puts("roda: falhou\r\n");
        return;
    };
    let place = match fs::place("prog", &image[..size]) {
        Some(fs::Place::Wrote) => "gravou",
        Some(fs::Place::Disk) => "disco",
        None => {
            uart::puts("roda: falhou\r\n");
            return;
        }
    };
    uart::write_fmt(format_args!("roda: {place}\r\n"));
    let mut stored = [0u8; 512];
    let Some(got) = fs::read_root("prog", &mut stored) else {
        uart::puts("roda: falhou\r\n");
        return;
    };
    let Some(code) = program(&stored[..got]) else {
        uart::puts("roda: falhou\r\n");
        return;
    };
    let Some(status) = execute(code) else {
        uart::puts("roda: falhou\r\n");
        return;
    };
    uart::write_fmt(format_args!("saida: {status}\r\n"));
}

pub fn run_name(name: &str) {
    if name.is_empty() {
        uart::puts("run: ausente\r\n");
        return;
    }
    uart::write_fmt(format_args!("run: {name}\r\n"));
    if !sd_ready() {
        uart::puts("run: sem cartao\r\n");
        return;
    }
    let mut stored = [0u8; 512];
    let Some(got) = fs::read_root(name, &mut stored) else {
        uart::puts("run: ausente\r\n");
        return;
    };
    let Some(code) = program(&stored[..got]) else {
        uart::puts("run: formato\r\n");
        return;
    };
    let Some(status) = execute(code) else {
        uart::puts("run: falhou\r\n");
        return;
    };
    uart::write_fmt(format_args!("run: ok {status}\r\n"));
}

fn execute(code: &[u8]) -> Option<u64> {
    syscall::begin_exit();
    if !user::launch(code.as_ptr(), code.len(), 0) {
        return None;
    }
    let status = syscall::exit_status();
    if status == u64::MAX {
        None
    } else {
        Some(status)
    }
}

fn sd_ready() -> bool {
    crate::sd::open()
}

fn build(dest: &mut [u8; 512]) -> Option<usize> {
    let start = core::ptr::addr_of!(app_code) as *const u8;
    let len = core::ptr::addr_of!(app_code_end) as usize - start as usize;
    if len == 0 || 16 + len > dest.len() {
        return None;
    }
    dest[0..4].copy_from_slice(b"RPEX");
    dest[4..8].copy_from_slice(&1u32.to_le_bytes());
    dest[8..12].copy_from_slice(&0u32.to_le_bytes());
    dest[12..16].copy_from_slice(&(len as u32).to_le_bytes());
    unsafe { core::ptr::copy_nonoverlapping(start, dest[16..].as_mut_ptr(), len) };
    Some(16 + len)
}

fn program(file: &[u8]) -> Option<&[u8]> {
    if file.len() < 16 || file[0..4] != *b"RPEX" {
        return None;
    }
    let version = u32::from_le_bytes([file[4], file[5], file[6], file[7]]);
    let entry = u32::from_le_bytes([file[8], file[9], file[10], file[11]]) as usize;
    let len = u32::from_le_bytes([file[12], file[13], file[14], file[15]]) as usize;
    if version != 1 || entry != 0 || len == 0 || 16 + len > file.len() {
        return None;
    }
    Some(&file[16..16 + len])
}
