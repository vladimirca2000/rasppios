//! MiniUART (UART1) do BCM2837, mapa do BCM2835 ARM Peripherals.
//!
//! No Pi 2/3 o endereço de barramento `0x7Enn_nnnn` aparece em
//! `0x3Fnn_nnnn` para o ARM. GPIO14/15 em ALT5 são TXD1/RXD1.
//!
//! No QEMU `raspi3b` este periférico é o segundo serial
//! (`-serial null -serial stdio`). No Pi 3B+, `enable_uart=1` o liga
//! aos pinos do conector, com o PL011 no Bluetooth.

use core::fmt;
use core::ptr::{read_volatile, write_volatile};

const PERIPH_BASE: usize = 0x3F00_0000;
const GPIO_BASE: usize = PERIPH_BASE + 0x20_0000;
const AUX_BASE: usize = PERIPH_BASE + 0x21_5000;

const GPFSEL1: usize = GPIO_BASE + 0x04;
const GPPUD: usize = GPIO_BASE + 0x94;
const GPPUDCLK0: usize = GPIO_BASE + 0x98;

const AUX_ENABLES: usize = AUX_BASE + 0x04;
const AUX_MU_IO: usize = AUX_BASE + 0x40;
const AUX_MU_IER: usize = AUX_BASE + 0x44;
const AUX_MU_IIR: usize = AUX_BASE + 0x48;
const AUX_MU_LCR: usize = AUX_BASE + 0x4C;
const AUX_MU_MCR: usize = AUX_BASE + 0x50;
const AUX_MU_LSR: usize = AUX_BASE + 0x54;
const AUX_MU_CNTL: usize = AUX_BASE + 0x60;
const AUX_MU_BAUD: usize = AUX_BASE + 0x68;

/// `core_freq` 250 MHz, 115200 8N1: `250e6 / (8 * 115200) - 1 = 270`.
const BAUD_DIV_115200: u32 = 270;

/// BCM2835 página 15: FIFO de recepção com ao menos um símbolo.
const LSR_RX_READY: u32 = 1 << 0;
/// BCM2835 página 15: a FIFO de transmissão aceita ao menos um byte.
const LSR_TX_EMPTY: u32 = 1 << 5;
/// BCM2835 página 13: escrever o bit 1 de `AUX_MU_IIR` esvazia a recepção.
const IIR_CLEAR_RX: u32 = 1 << 1;

/// Os endereços acima são MMIO do BCM2837, alinhados a 4, não objetos Rust.
///
/// # Safety
/// `addr` precisa ser um registrador deste módulo. A escrita é visível ao
/// periférico porque o ponteiro é volatile.
unsafe fn wr(addr: usize, value: u32) {
    unsafe { write_volatile(addr as *mut u32, value) };
}

/// # Safety
/// `addr` precisa ser um registrador deste módulo. A leitura não é eliminada
/// porque o ponteiro é volatile.
unsafe fn rd(addr: usize) -> u32 {
    unsafe { read_volatile(addr as *const u32) }
}

pub fn init() {
    unsafe {
        // Função ALT5 e pull-up antes de ligar o receptor (BCM2835 página 10).
        let mut sel = rd(GPFSEL1);
        sel &= !((0b111 << 12) | (0b111 << 15));
        sel |= (0b010 << 12) | (0b010 << 15);
        wr(GPFSEL1, sel);

        // Sequência de pull do BCM2835: GPPUD, 150 ciclos, GPPUDCLK, 150 ciclos.
        wr(GPPUD, 0b10);
        delay_cycles();
        wr(GPPUDCLK0, (1 << 14) | (1 << 15));
        delay_cycles();
        wr(GPPUD, 0);
        wr(GPPUDCLK0, 0);

        wr(AUX_ENABLES, 1);
        wr(AUX_MU_CNTL, 0);
        wr(AUX_MU_IER, 0);
        wr(AUX_MU_LCR, 3);
        wr(AUX_MU_MCR, 0);
        wr(AUX_MU_BAUD, BAUD_DIV_115200);
        wr(AUX_MU_IIR, IIR_CLEAR_RX);
        wr(AUX_MU_CNTL, 3);
    }
}

fn delay_cycles() {
    for _ in 0..150 {
        core::hint::spin_loop();
    }
}

pub fn putc(byte: u8) {
    unsafe {
        while (rd(AUX_MU_LSR) & LSR_TX_EMPTY) == 0 {
            core::hint::spin_loop();
        }
        wr(AUX_MU_IO, byte as u32);
    }
}

/// Bloqueia até a FIFO de recepção ter um símbolo e devolve os 8 bits de baixo.
pub fn getc() -> u8 {
    loop {
        if let Some(byte) = try_getc() {
            return byte;
        }
        core::hint::spin_loop();
    }
}

/// Um byte da FIFO, se já houver algum. Não espera.
pub fn try_getc() -> Option<u8> {
    unsafe {
        if (rd(AUX_MU_LSR) & LSR_RX_READY) == 0 {
            return None;
        }
        Some(rd(AUX_MU_IO) as u8)
    }
}

pub fn puts(s: &str) {
    for byte in s.bytes() {
        putc(byte);
    }
}

pub fn write_fmt(args: fmt::Arguments<'_>) {
    use fmt::Write;

    struct Out;
    impl Write for Out {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            puts(s);
            Ok(())
        }
    }
    let _ = Out.write_fmt(args);
}
