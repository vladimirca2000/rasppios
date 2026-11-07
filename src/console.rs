//! Console de diagnóstico na miniUART.
//!
//! Ecoa a linha digitada e responde a `help`, `el`, `nucleo`, `eco`, `ticks`, `mem`, `pagina`, `trava`, `tarefas`, `preempcao`, `falha` e `panico`.
//! A espera por caractere é por consulta à FIFO; interrupção fica para o marco 5.

use crate::exception;
use crate::mmu;
use crate::physmem;
use crate::sched;
use crate::sync;
use crate::timer;
use crate::uart;

const LINE_MAX: usize = 64;

pub fn run(el: u8, core: u8, arrived: [u8; 4]) -> ! {
    loop {
        uart::puts("rasppios> ");
        let mut buf = [0u8; LINE_MAX];
        let n = read_line(&mut buf);
        let line = core::str::from_utf8(&buf[..n]).unwrap_or("");
        reply(line, el, core, arrived);
    }
}

fn read_line(buf: &mut [u8]) -> usize {
    let mut n = 0;
    loop {
        match uart::getc() {
            b'\r' | b'\n' => {
                uart::puts("\r\n");
                return n;
            }
            0x08 | 0x7f => {
                if n > 0 {
                    n -= 1;
                    uart::puts("\x08 \x08");
                }
            }
            byte @ 0x20..=0x7e if n < buf.len() => {
                buf[n] = byte;
                n += 1;
                uart::putc(byte);
            }
            _ => {}
        }
    }
}

fn reply(line: &str, el: u8, core: u8, arrived: [u8; 4]) {
    let (cmd, rest) = split_cmd(line);
    match cmd {
        "" => {}
        "help" => {
            uart::puts(
                "comandos: help el nucleo eco ticks mem pagina trava tarefas preempcao falha panico\r\n",
            )
        }
        "el" => uart::write_fmt(format_args!("el: EL{el}\r\n")),
        "nucleo" => uart::write_fmt(format_args!(
            "nucleo: {core} chegada: 0={} 1={} 2={} 3={}\r\n",
            flag(arrived[0]),
            flag(arrived[1]),
            flag(arrived[2]),
            flag(arrived[3]),
        )),
        "eco" => uart::write_fmt(format_args!("eco: {rest}\r\n")),
        "ticks" => uart::write_fmt(format_args!(
            "ticks: {} clo: {}\r\n",
            timer::ticks(),
            timer::clo()
        )),
        "mem" => physmem::report(),
        "pagina" => mmu::exercise(),
        "trava" => sync::exercise(),
        "tarefas" => sched::exercise(),
        "preempcao" => sched::preempt(),
        "falha" => exception::provoke_brk(),
        "panico" => panic!("provocado"),
        other => uart::write_fmt(format_args!("desconhecido: {other}\r\n")),
    }
}

fn split_cmd(line: &str) -> (&str, &str) {
    let line = line.trim();
    let mut parts = line.splitn(2, char::is_whitespace);
    let cmd = parts.next().unwrap_or("");
    let rest = parts.next().unwrap_or("").trim();
    (cmd, rest)
}

fn flag(seen: u8) -> u8 {
    if seen == 0 { 0 } else { 1 }
}
