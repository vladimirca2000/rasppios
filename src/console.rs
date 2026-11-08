//! Console de diagnóstico na miniUART.
//!
//! Ecoa a linha digitada e responde a `help`, `ls`, `cat`, `echo`, `run` e aos comandos de diagnóstico.
//! A espera por caractere é por consulta à FIFO; interrupção fica para o marco 5.

use crate::exception;
use crate::fs;
use crate::gui;
use crate::load;
use crate::log;
use crate::mmu;
use crate::physmem;
use crate::sched;
use crate::sd;
use crate::sync;
use crate::syscall;
use crate::timer;
use crate::uart;
use crate::usb;
use crate::user;
use crate::video;

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
        if gui::poll() {
            gui::render();
        }
        // A troca de resolução pendente expira sozinha, sem exigir entrada:
        // sem isto, o modo errado ficaria na tela até a próxima tecla.
        if pending_expired() {
            expire_pending();
            uart::puts("rasppios> ");
        }
        let byte = if let Some(byte) = uart::try_getc() {
            Some(byte)
        } else if gui::ready() {
            None
        } else {
            usb::poll()
        };
        match byte {
            Some(b'\r' | b'\n') => {
                uart::puts("\r\n");
                return n;
            }
            Some(0x08 | 0x7f) => {
                if n > 0 {
                    n -= 1;
                    uart::puts("\x08 \x08");
                }
            }
            Some(byte @ 0x20..=0x7e) if n < buf.len() => {
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
    // Uma troca de resolução pendente consome a próxima linha: ela é a
    // confirmação (ou o cancelamento), independente do comando digitado.
    if pending_read().is_some() {
        resolve_pending(line.trim());
        return;
    }
    match cmd {
        "" => {}
        "help" => {
            uart::puts(
                "comandos: help ls cat echo run el nucleo eco ticks mem pagina trava tarefas preempcao usuario chamada bloco arquivos fsck stress verifica roda falha panico tela logs limites versao gfx modos resolucao\r\n",
            )
        }
        "ls" => fs::shell_ls(rest),
        "cat" => fs::shell_cat(rest),
        "echo" | "eco" => uart::write_fmt(format_args!("{cmd}: {rest}\r\n")),
        "run" => load::run_name(rest),
        "el" => uart::write_fmt(format_args!("el: EL{el}\r\n")),
        "nucleo" => uart::write_fmt(format_args!(
            "nucleo: {core} chegada: 0={} 1={} 2={} 3={}\r\n",
            flag(arrived[0]),
            flag(arrived[1]),
            flag(arrived[2]),
            flag(arrived[3]),
        )),
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
        "usuario" => user::exercise(),
        "chamada" => syscall::exercise(),
        "bloco" => sd::exercise(),
        "arquivos" | "arquivo" => fs::exercise(),
        "fsck" => fs::check(),
        "stress" => fs::stress(),
        "verifica" => {
            fs::check();
            fs::stress();
        }
        "roda" => load::exercise(),
        "tela" => {
            if video::exercise() {
                match video::active() {
                    Some(mode) => uart::write_fmt(format_args!(
                        "tela: ok {}x{} rgb565\r\n",
                        mode.width, mode.height
                    )),
                    None => uart::puts("tela: indisponivel\r\n"),
                }
            } else {
                uart::puts("tela: indisponivel\r\n");
            }
        }
        "logs" => log::print(),
        "limites" => log::print_limits(),
        "versao" => crate::version::print(),
        "gfx" => {
            crate::gfx::self_test();
        }
        "modos" => {
            crate::modes::report();
            crate::modes::self_test();
        }
        "resolucao" => resolution(rest),
        "falha" => match rest {
            "leitura" => sd::set_fault_mode(sd::FaultMode::FailNextRead),
            "escrita" => sd::set_fault_mode(sd::FaultMode::FailNextWrite),
            value => {
                let Some(count) = value.strip_prefix("apos ").and_then(|n| n.parse().ok()) else {
                    exception::provoke_brk();
                    return;
                };
                sd::set_fault_mode(sd::FaultMode::FailAfter(count));
            }
        },
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
    if seen == 0 {
        0
    } else {
        1
    }
}

/// `resolucao` mostra o modo ativo; `resolucao N` aplica o modo de índice
/// `N` da tabela e exige confirmação.
///
/// A confirmação é a própria tecla: enquanto a troca está pendente, qualquer
/// linha digitada confirma; `resolucao cancela` volta ao modo anterior. Se
/// nada chegar em `CONFIRM_TICKS`, o modo anterior é restaurado sozinho.
fn resolution(rest: &str) {
    if rest.is_empty() {
        match crate::video::active() {
            Some(mode) => uart::write_fmt(format_args!(
                "resolucao: atual {}x{} stride {}\r\n",
                mode.width, mode.height, mode.stride
            )),
            None => uart::puts("resolucao: sem video\r\n"),
        }
        return;
    }
    let Some(index) = rest.parse::<usize>().ok() else {
        uart::puts("resolucao: indice invalido\r\n");
        return;
    };
    let Ok(mode) = crate::modes::select(index) else {
        uart::puts("resolucao: modo nao suportado\r\n");
        return;
    };
    match crate::modes::apply(mode) {
        Some(pending) => {
            // O buffer novo vem com memória não inicializada. Sem redesenhar
            // imediatamente, a tela mostra lixo com o stride antigo — o
            // padrão de colunas repetidas observado no QEMU e no Pi.
            repaint();
            uart::write_fmt(format_args!(
                "resolucao: {}x{} aplicada; digite qualquer linha para confirmar ou 'cancela' para voltar ({} s)\r\n",
                pending.after.width,
                pending.after.height,
                crate::modes::CONFIRM_TICKS / 100,
            ));
            pending_write(Some(pending));
        }
        None => uart::puts("resolucao: falhou, modo anterior mantido\r\n"),
    }
}

/// Limpa e redesenha a GUI no modo atual.
///
/// Chamado depois de qualquer troca de modo, porque o firmware entrega um
/// buffer novo sem conteúdo e o anterior deixa de valer.
fn repaint() {
    crate::video::clear(crate::video::NAVY);
    let _ = crate::video::flush();
    if crate::gui::ready() {
        crate::gui::render();
    }
}

/// Resolve a troca pendente com a linha digitada pelo usuário.
fn resolve_pending(line: &str) {
    let Some(pending) = pending_take() else {
        return;
    };
    if line == "cancela" {
        if crate::modes::cancel(&pending) {
            repaint();
            uart::puts("resolucao: modo anterior restaurado\r\n");
        } else {
            uart::puts("resolucao: falha ao restaurar\r\n");
        }
        return;
    }
    // A expiração já é tratada em `read_line`, que não espera entrada. Se a
    // linha chegou, ela confirma.
    uart::write_fmt(format_args!(
        "resolucao: confirmada {}x{}\r\n",
        pending.after.width, pending.after.height
    ));
}

/// Troca de resolução aguardando confirmação do usuário.
///
/// O console é single-threaded (só o núcleo 0 o usa), então `static mut`
/// com acesso pelas funções abaixo é seguro e evita `unsafe` espalhado.
static mut PENDING_CHECK: Option<crate::modes::Pending> = None;

fn pending_read() -> Option<crate::modes::Pending> {
    unsafe { core::ptr::read_volatile(core::ptr::addr_of!(PENDING_CHECK)) }
}

fn pending_write(value: Option<crate::modes::Pending>) {
    unsafe { core::ptr::write_volatile(core::ptr::addr_of_mut!(PENDING_CHECK), value) }
}

fn pending_take() -> Option<crate::modes::Pending> {
    let value = pending_read();
    pending_write(None);
    value
}

/// Verdadeiro quando há uma troca pendente cujo prazo já passou.
fn pending_expired() -> bool {
    match pending_read() {
        Some(pending) => pending.expired(),
        None => false,
    }
}

/// Restaura o modo anterior de uma troca expirada, sem entrada do usuário.
fn expire_pending() {
    let Some(pending) = pending_take() else {
        return;
    };
    if crate::modes::cancel(&pending) {
        repaint();
        uart::puts("\r\nresolucao: expirou; modo anterior restaurado\r\n");
    } else {
        uart::puts("\r\nresolucao: expirou; falha ao restaurar\r\n");
    }
}
