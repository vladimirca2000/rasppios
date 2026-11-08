//! Cartão SD pelo SDHOST do BCM2837.
//!
//! A base é `0x3F202000` (`MMCI0_OFFSET` 0x202000). Os deslocamentos são os
//! de `hw/sd/bcm2835_sdhost.c` do QEMU 8.2.2: `SDCMD` 0x00, `SDARG` 0x04,
//! `SDCDIV` 0x0c, `SDRSP0` 0x10, `SDHSTS` 0x20, `SDVDD` 0x30, `SDEDM` 0x34,
//! `SDHCFG` 0x38, `SDHBCT` 0x3c, `SDDATA` 0x40 e `SDHBLC` 0x50.
//!
//! No QEMU o cartão começa no Arasan. A função ALT0 (`4`) nos GPIO 48–53,
//! como em `bcm2835_gpio.c`, entrega o cartão a este controlador. No Pi 3B+
//! o firmware já deixa esses pinos em ALT0: o microSD está no SDHOST.
//!
//! O comando `bloco` lê o bloco 0, grava `0xA5` no bloco 1, confere e
//! devolve os bytes originais.

use core::ptr::{read_volatile, write_volatile};

use crate::timer;
use crate::uart;
use crate::log;

const GPIO: usize = 0x3F20_0000;
const HOST: usize = 0x3F20_2000;

const SDCMD: usize = 0x00;
const SDARG: usize = 0x04;
const SDTOUT: usize = 0x08;
const SDCDIV: usize = 0x0c;
const SDRSP0: usize = 0x10;
const SDHSTS: usize = 0x20;
const SDVDD: usize = 0x30;
const SDEDM: usize = 0x34;
const SDHCFG: usize = 0x38;
const SDHBCT: usize = 0x3c;
const SDDATA: usize = 0x40;
const SDHBLC: usize = 0x50;

const CMD_NEW: u32 = 0x8000;
const CMD_FAIL: u32 = 0x4000;
const CMD_BUSY: u32 = 0x800;
const CMD_NO_RESP: u32 = 0x400;
const CMD_LONG: u32 = 0x200;
const CMD_WRITE: u32 = 0x80;
const CMD_READ: u32 = 0x40;

const STS_ERROR: u32 = 0xf8;
const STS_DATA: u32 = 0x01;
const CFG_SLOW: u32 = 1 << 3;
const CFG_BUSY: u32 = 1 << 10;
const EDM_FIFO_SHIFT: u32 = 4;
const EDM_FIFO_MASK: u32 = 0x1f;
const EDM_FSM_MASK: u32 = 0xf;
const EDM_FORCE_DATA: u32 = 1 << 19;
const FSM_IDENT: u32 = 0x0;
const FSM_DATA: u32 = 0x1;
const FSM_READ_WAIT: u32 = 0x4;
const FSM_WRITE_START: u32 = 0xa;

const BLOCK: usize = 512;
const WORDS: usize = BLOCK / 4;

static mut ORIG: [u8; BLOCK] = [0; BLOCK];
static mut TRIAL: [u8; BLOCK] = [0; BLOCK];
static mut GOT: [u8; BLOCK] = [0; BLOCK];
static mut BLOCK_ADDR: bool = false;
static mut RCA: u32 = 0;

#[derive(Clone, Copy)]
pub enum FaultMode {
    None,
    FailNextRead,
    FailNextWrite,
    FailAfter(u32),
}

static mut FAULT: FaultMode = FaultMode::None;

pub fn set_fault_mode(mode: FaultMode) {
    unsafe { FAULT = mode };
    crate::log::warn("sd-fault-configured");
}

pub(crate) fn open() -> bool {
    let ready = init();
    if ready {
        log::info("sd-open");
    } else {
        log::error("sd-open-fail");
    }
    ready
}

pub fn exercise() {
    if !init() {
        uart::puts("bloco: sem cartao\r\n");
        return;
    }
    let got = unsafe { &mut *core::ptr::addr_of_mut!(GOT) };
    let orig = unsafe { &mut *core::ptr::addr_of_mut!(ORIG) };
    let trial = unsafe { &mut *core::ptr::addr_of_mut!(TRIAL) };
    if !read_block(0, got) {
        uart::puts("bloco: falhou\r\n");
        return;
    }
    let head = u32::from_le_bytes([got[0], got[1], got[2], got[3]]);
    uart::write_fmt(format_args!("leitura: {head:08x}\r\n"));
    if !read_block(1, orig) {
        uart::puts("bloco: falhou\r\n");
        return;
    }
    trial.fill(0xa5);
    if !write_block(1, trial) || !read_block(1, got) || got != trial {
        uart::puts("bloco: falhou\r\n");
        return;
    }
    uart::puts("escrita: ok\r\n");
    if !write_block(1, orig) || !read_block(1, got) || got != orig {
        uart::puts("bloco: falhou\r\n");
        return;
    }
    uart::puts("restauro: ok\r\n");
}

fn init() -> bool {
    select_sdhost();
    // O Linux desliga a alimentação, limita a FIFO (defeito do silício)
    // e só então liga de novo. SDTOUT em zero encerra a escrita cedo demais.
    wr(SDVDD, 0);
    tune_fifo();
    delay_us(20_000);
    wr(SDVDD, 1);
    delay_us(20_000);
    wr(SDTOUT, 0x00f0_0000);
    wr(SDCDIV, 0x7ff);
    wr(SDHCFG, CFG_SLOW | CFG_BUSY);
    wr(SDHSTS, 0x7ff);
    if !command(0, 0, CMD_NO_RESP) {
        return false;
    }
    delay_us(1_000);
    if !command(8, 0x1aa, 0) {
        return false;
    }
    if rd(SDRSP0) & 0xfff != 0x1aa {
        return false;
    }
    let mut ocr = 0u32;
    let mut ready = false;
    for _ in 0..50 {
        if !command(55, 0, 0) || !command(41, 0x4030_0000, 0) {
            return false;
        }
        ocr = rd(SDRSP0);
        if ocr & (1 << 31) != 0 {
            ready = true;
            break;
        }
        delay_us(10_000);
    }
    if !ready {
        return false;
    }
    unsafe { BLOCK_ADDR = ocr & (1 << 30) != 0 };
    if !command(2, 0, CMD_LONG) || !command(3, 0, 0) {
        return false;
    }
    let rca = rd(SDRSP0) & 0xffff_0000;
    unsafe { RCA = rca };
    if !command(7, rca, CMD_BUSY) {
        return false;
    }
    if unsafe { !BLOCK_ADDR } && !command(16, BLOCK as u32, 0) {
        return false;
    }
    true
}

pub(crate) fn read_block(block: u32, dest: &mut [u8; BLOCK]) -> bool {
    if fault(true) {
        crate::log::error("sd-read-fault");
        return false;
    }
    wr(SDHBCT, BLOCK as u32);
    wr(SDHBLC, 1);
    wr(SDHSTS, 0x7ff);
    if !command(17, address(block), CMD_READ) {
        return false;
    }
    for index in 0..WORDS {
        if !wait_fifo(true) {
            return false;
        }
        let word = rd(SDDATA);
        dest[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    finish_fsm()
}

pub(crate) fn write_block(block: u32, src: &[u8; BLOCK]) -> bool {
    if fault(false) {
        crate::log::error("sd-write-fault");
        return false;
    }
    wr(SDHBCT, BLOCK as u32);
    wr(SDHBLC, 1);
    wr(SDHSTS, 0x7ff);
    if !command(24, address(block), CMD_WRITE | CMD_BUSY) {
        return false;
    }
    for index in 0..WORDS {
        if !wait_fifo(false) {
            return false;
        }
        let word = u32::from_le_bytes([
            src[index * 4],
            src[index * 4 + 1],
            src[index * 4 + 2],
            src[index * 4 + 3],
        ]);
        wr(SDDATA, word);
    }
    finish_fsm() && card_tran()
}

fn fault(read: bool) -> bool {
    unsafe {
        match FAULT {
            FaultMode::None => false,
            FaultMode::FailNextRead if read => {
                FAULT = FaultMode::None;
                true
            }
            FaultMode::FailNextWrite if !read => {
                FAULT = FaultMode::None;
                true
            }
            FaultMode::FailAfter(0) => {
                FAULT = FaultMode::None;
                true
            }
            FaultMode::FailAfter(left) => {
                FAULT = FaultMode::FailAfter(left - 1);
                false
            }
            _ => false,
        }
    }
}

/// O controlador às vezes para em espera de escrita. O bit 19 empurra a FSM
/// de volta a modo de dados, como faz `bcm2835_wait_transfer_complete`.
fn finish_fsm() -> bool {
    let start = timer::clo();
    loop {
        let edm = rd(SDEDM);
        let fsm = edm & EDM_FSM_MASK;
        if fsm == FSM_IDENT || fsm == FSM_DATA {
            return true;
        }
        if fsm == FSM_READ_WAIT || fsm == FSM_WRITE_START {
            wr(SDEDM, edm | EDM_FORCE_DATA);
            return true;
        }
        if timer::clo().wrapping_sub(start) > 500_000 {
            return false;
        }
    }
}

/// CMD13 até o cartão sair de programação e voltar a `tran` (estado 4).
fn card_tran() -> bool {
    let rca = unsafe { RCA };
    let start = timer::clo();
    loop {
        if command(13, rca, 0) {
            let state = (rd(SDRSP0) >> 9) & 0xf;
            if state == 4 {
                return true;
            }
        }
        if timer::clo().wrapping_sub(start) > 500_000 {
            return false;
        }
        delay_us(1_000);
    }
}

fn tune_fifo() {
    let mut edm = rd(SDEDM);
    edm &= !((0x1f << 14) | (0x1f << 9));
    edm |= (4 << 14) | (4 << 9);
    wr(SDEDM, edm);
}

fn address(block: u32) -> u32 {
    if unsafe { BLOCK_ADDR } {
        block
    } else {
        block.wrapping_mul(BLOCK as u32)
    }
}

fn command(index: u32, arg: u32, flags: u32) -> bool {
    wr(SDHSTS, 0x7ff);
    wr(SDARG, arg);
    wr(SDCMD, (index & 0x3f) | CMD_NEW | flags);
    let start = timer::clo();
    loop {
        let cmd = rd(SDCMD);
        if cmd & CMD_NEW == 0 {
            let status = rd(SDHSTS);
            return cmd & CMD_FAIL == 0 && status & STS_ERROR == 0;
        }
        if timer::clo().wrapping_sub(start) > 200_000 {
            return false;
        }
    }
}

fn wait_fifo(want_data: bool) -> bool {
    let start = timer::clo();
    loop {
        if rd(SDHSTS) & STS_ERROR != 0 {
            return false;
        }
        let level = (rd(SDEDM) >> EDM_FIFO_SHIFT) & EDM_FIFO_MASK;
        if want_data && level > 0 {
            return true;
        }
        if !want_data && level < 16 {
            return true;
        }
        if rd(SDHSTS) & STS_DATA != 0 && want_data {
            return true;
        }
        if timer::clo().wrapping_sub(start) > 500_000 {
            return false;
        }
    }
}

/// ALT0 nos GPIO 48–53. No QEMU isso move o cartão para o SDHOST.
fn select_sdhost() {
    let mut sel4 = gpio(0x10);
    sel4 &= !((7 << 24) | (7 << 27));
    sel4 |= (4 << 24) | (4 << 27);
    gpio_w(0x10, sel4);
    let mut sel5 = gpio(0x14);
    sel5 &= !0xfff;
    sel5 |= (4 << 0) | (4 << 3) | (4 << 6) | (4 << 9);
    gpio_w(0x14, sel5);
}

fn gpio(off: usize) -> u32 {
    unsafe { read_volatile((GPIO + off) as *const u32) }
}

fn gpio_w(off: usize, value: u32) {
    unsafe { write_volatile((GPIO + off) as *mut u32, value) }
}

fn rd(off: usize) -> u32 {
    unsafe { read_volatile((HOST + off) as *const u32) }
}

fn wr(off: usize, value: u32) {
    unsafe { write_volatile((HOST + off) as *mut u32, value) }
}

fn delay_us(us: u32) {
    let start = timer::clo();
    while timer::clo().wrapping_sub(start) < us {
        core::hint::spin_loop();
    }
}
