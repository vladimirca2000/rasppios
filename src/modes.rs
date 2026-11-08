//! Descoberta de modos de vídeo.
//!
//! Pergunta ao firmware o que o conjunto monitor + VideoCore oferece e
//! mantém uma lista curta de modos que o projeto aceita. A lista é
//! conservadora de propósito: só entram modos que o restante do sistema
//! sabe desenhar (RGB565, stride múltiplo de 2 e área dentro do contrato de
//! `gfx`). Um modo desconhecido nunca é usado às cegas.
//!
//! A consulta usa o mailbox de propriedades já validado pelo Marco 18:
//! a tag `GET_DISPLAY_DIMENSIONS` devolve largura e altura correntes.

use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};

use crate::gfx;
use crate::uart;
use crate::video::ActiveMode;

const MBOX_READ: usize = 0x3F00_B880;
const MBOX_STATUS: usize = 0x3F00_B898;
const MBOX_WRITE: usize = 0x3F00_B8A0;
const MBOX_FULL: u32 = 1 << 31;
const MBOX_EMPTY: u32 = 1 << 30;
const MBOX_CHANNEL: u32 = 8;
const BUS_ALIAS: u32 = 0xC000_0000;
const CACHE_LINE: usize = 64;

/// `GET_PHYSICAL_WIDTH_HEIGHT`. O valor `0x0004_8003` é a tag de *escrita*
/// correspondente e não devolve as dimensões.
const TAG_GET_DISPLAY_DIMENSIONS: u32 = 0x0004_0003;
const TAG_GET_PITCH: u32 = 0x0004_0008;

pub const MAX_MODES: usize = 6;

/// Modos aceitos pelo projeto, do menor para o maior.
///
/// Todos usam RGB565. A ordem importa: o índice 0 é o modo seguro para
/// recuperação, porque a menor área é a mais provável de caber quando algo
/// deu errado.
pub const SAFE_MODES: [gfx::Mode; MAX_MODES] = [
    gfx::Mode::new(640, 480, 1280),
    gfx::Mode::new(800, 600, 1600),
    gfx::Mode::new(1024, 768, 2048),
    gfx::Mode::new(1280, 720, 2560),
    gfx::Mode::new(1280, 1024, 2560),
    gfx::Mode::new(1920, 1080, 3840),
];

#[derive(Clone, Copy)]
pub struct Discovered {
    /// Dimensões que o firmware reporta como ativas.
    pub current_width: u32,
    pub current_height: u32,
    /// Stride corrente; zero quando o firmware não respondeu.
    pub current_stride: u32,
    /// Quantos modos de `SAFE_MODES` passam pelo contrato.
    pub supported: usize,
    /// Verdadeiro quando o mailbox respondeu a todas as consultas.
    pub mailbox_ok: bool,
}

static mut LAST: Discovered = Discovered {
    current_width: 0,
    current_height: 0,
    current_stride: 0,
    supported: 0,
    mailbox_ok: false,
};

static mut MESSAGE: [u32; 12] = [0; 12];

/// Consulta o firmware e monta a lista de modos aceitos.
pub fn discover() -> Discovered {
    let (mailbox_ok, width, height) = query_display();
    let stride = query_pitch(width);

    let mut supported = 0usize;
    for mode in SAFE_MODES.iter() {
        if mode.validate().is_ok() {
            supported += 1;
        }
    }

    let found = Discovered {
        current_width: width,
        current_height: height,
        current_stride: stride,
        supported,
        mailbox_ok,
    };
    unsafe {
        write_volatile(addr_of_mut!(LAST), found);
    }
    found
}

/// Máscara dos modos que passam pelo contrato, calculada em tempo de
/// compilação. Não depende do firmware: se um modo da tabela é válido para o
/// contrato, ele pode ser tentado; quem recusa de fato é a negociação.
const fn valid_mask() -> u32 {
    let mut mask = 0u32;
    let mut index = 0;
    while index < MAX_MODES {
        // `Mode::validate` não é `const`, então a checagem é repetida aqui
        // com os mesmos limites do contrato.
        let mode = SAFE_MODES[index];
        let minimum = mode.width * 2;
        let ok = mode.width > 0
            && mode.height > 0
            && mode.width <= gfx::MAX_WIDTH
            && mode.height <= gfx::MAX_HEIGHT
            && mode.stride >= minimum
            && mode.stride <= gfx::MAX_STRIDE
            && mode.stride % 2 == 0;
        if ok {
            mask |= 1 << index;
        }
        index += 1;
    }
    mask
}

const VALID_MASK: u32 = valid_mask();

/// Verdadeiro quando o modo da tabela é aceitável pelo contrato.
///
/// Não consulta o firmware nem estado em cache: `resolucao N` precisa
/// funcionar logo depois do boot, sem exigir que `modos` tenha rodado antes.
pub fn is_supported(index: usize) -> bool {
    index < MAX_MODES && index < 32 && VALID_MASK & (1 << index) != 0
}

/// Impressão usada pelo comando `modos` e pelo boot.
pub fn report() {
    let found = discover();
    if found.mailbox_ok {
        uart::write_fmt(format_args!(
            "modos: atual {}x{} stride {}\r\n",
            found.current_width, found.current_height, found.current_stride
        ));
    } else {
        uart::puts("modos: firmware nao respondeu\r\n");
    }
    uart::write_fmt(format_args!(
        "modos: {} aceitos de {}\r\n",
        found.supported, MAX_MODES
    ));
    for (index, mode) in SAFE_MODES.iter().enumerate() {
        if is_supported(index) {
            let marker = if mode.width == found.current_width && mode.height == found.current_height
            {
                '*'
            } else {
                ' '
            };
            uart::write_fmt(format_args!(
                "modo[{index}]{marker} {}x{} stride {} buffer {}\r\n",
                mode.width,
                mode.height,
                mode.stride,
                mode.buffer_len()
            ));
        }
    }
    if !is_supported(0) {
        uart::puts("modos: sem modo seguro\r\n");
    }
}

/// Devolve `(respondeu, largura, altura)`.
fn query_display() -> (bool, u32, u32) {
    unsafe {
        let words = addr_of_mut!(MESSAGE) as *mut u32;
        for index in 0..12 {
            write_volatile(words.add(index), 0);
        }
        // 8 palavras: cabeçalho (2), tag de 8 bytes (5) e tag final (1).
        write_volatile(words, 8 * 4);
        write_volatile(words.add(1), 0);
        write_volatile(words.add(2), TAG_GET_DISPLAY_DIMENSIONS);
        write_volatile(words.add(3), 8);
        write_volatile(words.add(4), 0);
        write_volatile(words.add(5), 0);
        write_volatile(words.add(6), 0);
    }
    if !call() {
        return (false, 0, 0);
    }
    unsafe {
        let words = addr_of!(MESSAGE) as *const u32;
        // Palavra 3 é o tamanho do buffer da tag; o código de resposta fica
        // na palavra 4, como em `TAG_*` de `video.rs`.
        let tag = read_volatile(words.add(2));
        let code = read_volatile(words.add(4));
        if tag != TAG_GET_DISPLAY_DIMENSIONS || code & (1 << 31) == 0 {
            return (false, 0, 0);
        }
        let width = read_volatile(words.add(5));
        let height = read_volatile(words.add(6));
        if width == 0 || height == 0 || width > gfx::MAX_WIDTH || height > gfx::MAX_HEIGHT {
            return (false, 0, 0);
        }
        (true, width, height)
    }
}

/// Pergunta o stride corrente para `width`. O firmware devolve zero quando
/// o modo não está ativo; nesse caso o valor é derivado do formato.
fn query_pitch(width: u32) -> u32 {
    unsafe {
        let words = addr_of_mut!(MESSAGE) as *mut u32;
        for index in 0..12 {
            write_volatile(words.add(index), 0);
        }
        // 7 palavras: cabeçalho (2), tag de 4 bytes (4) e tag final (1).
        write_volatile(words, 7 * 4);
        write_volatile(words.add(1), 0);
        write_volatile(words.add(2), TAG_GET_PITCH);
        write_volatile(words.add(3), 4);
        write_volatile(words.add(4), 0);
        write_volatile(words.add(5), 0);
    }
    if !call() {
        return default_stride(width);
    }
    unsafe {
        let words = addr_of!(MESSAGE) as *const u32;
        if read_volatile(words.add(2)) != TAG_GET_PITCH
            || read_volatile(words.add(4)) & (1 << 31) == 0
        {
            return default_stride(width);
        }
        let stride = read_volatile(words.add(5));
        if stride == 0 || stride > gfx::MAX_STRIDE || stride % gfx::BYTES_PER_PIXEL as u32 != 0 {
            return default_stride(width);
        }
        stride
    }
}

const fn default_stride(width: u32) -> u32 {
    width * gfx::BYTES_PER_PIXEL as u32
}

/// Envia a mensagem e espera a resposta do mesmo canal.
fn call() -> bool {
    cache_clean_message();
    if !wait(|status| status & MBOX_FULL == 0) {
        return false;
    }
    let message = addr_of!(MESSAGE) as usize;
    let bus = ((message as u32) | BUS_ALIAS) & !0xf;
    unsafe { write_volatile(MBOX_WRITE as *mut u32, bus | MBOX_CHANNEL) };
    dsb();

    if !wait(|status| status & MBOX_EMPTY == 0) {
        return false;
    }
    let reply = unsafe { read_volatile(MBOX_READ as *const u32) };
    dsb();
    if reply & 0xf != MBOX_CHANNEL {
        return false;
    }
    cache_invalidate_message();
    true
}

fn wait(ready: fn(u32) -> bool) -> bool {
    for _ in 0..1_000_000u32 {
        let status = unsafe { read_volatile(MBOX_STATUS as *const u32) };
        if ready(status) {
            return true;
        }
    }
    false
}

fn dsb() {
    unsafe { core::arch::asm!("dsb sy", options(nostack, preserves_flags)) };
}

fn cache_clean_message() {
    cache_message(false);
}

fn cache_invalidate_message() {
    cache_message(true);
}

fn cache_message(invalidate: bool) {
    let start = addr_of!(MESSAGE) as usize & !(CACHE_LINE - 1);
    let end = (addr_of!(MESSAGE) as usize + core::mem::size_of_val(unsafe { &*addr_of!(MESSAGE) })
        + CACHE_LINE - 1)
        & !(CACHE_LINE - 1);
    let mut line = start;
    while line < end {
        unsafe {
            if invalidate {
                core::arch::asm!("dc ivac, {line}", line = in(reg) line, options(nostack));
            } else {
                core::arch::asm!("dc civac, {line}", line = in(reg) line, options(nostack));
            }
        }
        line += CACHE_LINE;
    }
    unsafe { core::arch::asm!("dsb sy", options(nostack, preserves_flags)) };
}

/// Aceita um modo da lista pelo índice. Só valida; a aplicação de fato é
/// responsabilidade do serviço de vídeo, que troca o buffer e remapeia.
pub fn select(index: usize) -> Result<gfx::Mode, gfx::Error> {
    if index >= MAX_MODES {
        return Err(gfx::Error::TooLarge);
    }
    let mode = SAFE_MODES[index];
    mode.validate()?;
    if !is_supported(index) {
        return Err(gfx::Error::TooLarge);
    }
    Ok(mode)
}

/// Prazo, em ticks, para confirmar uma troca de modo.
///
/// A 100 Hz, 1000 ticks são 10 s: tempo para uma pessoa olhar o monitor e
/// digitar. A janela anterior de 1,5 s era curta demais e fazia o comando
/// seguinte do usuário expirar a troca em vez de confirmá-la.
pub const CONFIRM_TICKS: u64 = 1000;

/// Troca de modo aguardando confirmação.
///
/// Se o monitor não sincronizar, o usuário não vê a tela e não consegue
/// confirmar. Por isso `apply` grava o modo anterior e `expired` permite que
/// o chamador restaure o antigo sem intervenção.
#[derive(Clone, Copy)]
pub struct Pending {
    pub before: ActiveMode,
    pub after: ActiveMode,
    pub ticks: u64,
}

impl Pending {
    pub fn expired(&self) -> bool {
        crate::timer::ticks().wrapping_sub(self.ticks) > CONFIRM_TICKS
    }

    /// Volta ao modo anterior.
    pub fn rollback(&self) -> Option<ActiveMode> {
        crate::video::init_mode(self.before.as_mode()).ok()
    }
}

/// Aplica `mode` e deixa a mudança pendente de confirmação.
pub fn apply(mode: gfx::Mode) -> Option<Pending> {
    mode.validate().ok()?;
    let before = crate::video::active()?;
    if before.as_mode() == mode {
        return None;
    }
    let after = crate::video::init_mode(mode).ok()?;
    Some(Pending {
        before,
        after,
        ticks: crate::timer::ticks(),
    })
}

/// Restaura o modo anterior de uma troca pendente.
pub fn cancel(pending: &Pending) -> bool {
    pending.rollback().is_some()
}

/// Exercita a descoberta e a seleção sem tocar no framebuffer.
pub fn self_test() -> bool {
    let mut failures = 0u32;

    macro_rules! check {
        ($label:literal, $ok:expr) => {
            if !$ok {
                failures += 1;
            }
            uart::write_fmt(format_args!(
                "modos {}: {}\r\n",
                $label,
                if $ok { "ok" } else { "falhou" }
            ));
        };
    }

    // Todos os modos da tabela passam pelo contrato.
    let mut all_valid = true;
    for mode in SAFE_MODES.iter() {
        if mode.validate().is_err() || mode.pixel_format != gfx::FORMAT {
            all_valid = false;
        }
    }
    check!("tabela valida", all_valid);

    // A ordem é crescente em área, e o primeiro é o modo seguro.
    let mut ordered = true;
    let mut previous = 0usize;
    for mode in SAFE_MODES.iter() {
        let area = mode.buffer_len();
        if area <= previous {
            ordered = false;
        }
        previous = area;
    }
    check!("ordem crescente", ordered);
    check!(
        "modo seguro 640x480",
        SAFE_MODES[0].width == 640 && SAFE_MODES[0].height == 480
    );
    check!("modo seguro aceito", select(0).is_ok());
    check!("indice fora da tabela", select(MAX_MODES).is_err());
    check!("tabela no limite do contrato", MAX_MODES <= 8);

    let found = discover();
    check!("descoberta executada", found.supported > 0);
    if found.mailbox_ok {
        check!(
            "atual dentro do contrato",
            found.current_width <= gfx::MAX_WIDTH && found.current_height <= gfx::MAX_HEIGHT
        );
        check!(
            "stride do contrato",
            found.current_stride % gfx::BYTES_PER_PIXEL as u32 == 0
        );
    }

    if failures == 0 {
        uart::puts("modos: descoberta ok\r\n");
    } else {
        uart::write_fmt(format_args!("modos: {failures} falhas\r\n"));
    }
    failures == 0
}
