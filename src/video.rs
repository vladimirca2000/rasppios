//! Inicialização do framebuffer RGB565 pelo mailbox de propriedades.

use core::mem::size_of;
use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};

use crate::{gfx, mmu, uart};

const MBOX_READ: usize = 0x3F00_B880;
const MBOX_STATUS: usize = 0x3F00_B898;
const MBOX_WRITE: usize = 0x3F00_B8A0;
const MBOX_FULL: u32 = 1 << 31;
const MBOX_EMPTY: u32 = 1 << 30;
const MBOX_CHANNEL: u32 = 8;
const BUS_ALIAS: u32 = 0xC000_0000;
const RAM_END: u64 = 0x3C00_0000;
const QEMU_FRAMEBUFFER_END: u64 = 0x3F00_0000;
const CACHE_LINE: usize = 64;

const TAG_PHYSICAL_SIZE: u32 = 0x0004_8003;
const TAG_VIRTUAL_SIZE: u32 = 0x0004_8004;
const TAG_DEPTH: u32 = 0x0004_8005;
const TAG_ALLOCATE_BUFFER: u32 = 0x0004_0001;
const TAG_RELEASE_BUFFER: u32 = 0x0004_8001;
const TAG_PITCH: u32 = 0x0004_0008;

pub const BLACK: u16 = 0x0000;
pub const NAVY: u16 = 0x000F;
pub const WHITE: u16 = 0xFFFF;
pub const RED: u16 = 0xF800;
pub const GREEN: u16 = 0x07E0;
pub const BLUE: u16 = 0x001F;

/// Resultado de uma negociação bem-sucedida com o firmware.
struct Negotiated {
    start: u64,
    size: usize,
    pitch: u32,
    width: u32,
    height: u32,
    /// Handle de barramento do buffer, usado para liberá-lo depois.
    bus: u32,
}

/// Modo publicado no framebuffer.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ActiveMode {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
}

impl ActiveMode {
    pub const fn as_mode(&self) -> gfx::Mode {
        gfx::Mode::new(self.width, self.height, self.stride)
    }
}

#[repr(C, align(16))]
struct FramebufferMessage {
    words: [u32; 35],
}

static mut MESSAGE: FramebufferMessage = FramebufferMessage { words: [0; 35] };
static mut AVAILABLE: bool = false;
static mut BUFFER: u64 = 0;
static mut BUFFER_SIZE: usize = 0;
static mut PITCH: u32 = 0;
static mut WIDTH: u32 = 0;
static mut HEIGHT: u32 = 0;
/// Handle de barramento do buffer em uso, necessário para liberá-lo.
static mut BUFFER_BUS: u32 = 0;

/// Modos tentados no boot, em ordem de preferência.
///
/// Full HD primeiro, depois HD, e por fim o modo seguro de 640x480. Só entra
/// na lista se o firmware entregar o buffer: um monitor que não aceite
/// 1920x1080 faz o kernel cair para 1280x720 sem intervenção.
const BOOT_MODES: [gfx::Mode; 3] = [
    gfx::Mode::new(1920, 1080, 3840),
    gfx::Mode::new(1280, 720, 2560),
    gfx::Mode::new(640, 480, 1280),
];

pub fn init() -> bool {
    for mode in BOOT_MODES {
        if let Ok(active) = init_mode(mode) {
            uart::write_fmt(format_args!(
                "video: {}x{} rgb565 stride {}\r\n",
                active.width, active.height, active.stride
            ));
            return true;
        }
        uart::write_fmt(format_args!(
            "video: {}x{} recusado; tentando o proximo\r\n",
            mode.width, mode.height
        ));
    }
    false
}

/// Negocia `mode`, mapeia o buffer e publica o estado. Em falha, o estado
/// anterior é restaurado negociando-o de novo, porque o firmware pode ter
/// desalocado o buffer antigo ao atender o pedido.
pub fn init_mode(mode: gfx::Mode) -> Result<ActiveMode, gfx::Error> {
    mode.validate()?;
    if mode.pixel_format != gfx::FORMAT {
        return Err(gfx::Error::TooLarge);
    }
    let previous = active();
    if let Some(state) = negotiate(mode.width, mode.height).and_then(install) {
        return Ok(state);
    }
    // O pedido falhou. Tenta voltar ao modo que estava ativo.
    if let Some(old) = previous {
        if let Some(_state) = negotiate(old.width, old.height).and_then(install) {
            return Err(gfx::Error::BufferTooSmall);
        }
        // Nem o modo anterior voltou: marca o vídeo como indisponível e
        // desfaz o mapeamento, para não deixar tradução privilegiada
        // sobrando sobre memória que não é mais framebuffer.
        mmu::unmap_framebuffer();
        unsafe { write_volatile(addr_of_mut!(AVAILABLE), false) };
    }
    Err(gfx::Error::BufferTooSmall)
}

/// Modo atualmente publicado.
pub fn active() -> Option<ActiveMode> {
    unsafe {
        if !read_volatile(addr_of!(AVAILABLE)) {
            return None;
        }
        Some(ActiveMode {
            width: read_volatile(addr_of!(WIDTH)),
            height: read_volatile(addr_of!(HEIGHT)),
            stride: read_volatile(addr_of!(PITCH)),
        })
    }
}

/// Mapeia e publica o resultado de `negotiate`. Devolve o modo instalado.
fn install(state: Negotiated) -> Option<ActiveMode> {
    let Negotiated { start, size, pitch, width, height, bus } = state;
    if !mmu::map_framebuffer(start, size) {
        // O buffer pode cair sobre a janela do programa de usuário em
        // `0x3C200000`. Nesse caso o modo não é utilizável por este kernel,
        // porque o EL0 perderia a própria página.
        let end = start.saturating_add(size as u64);
        if start < mmu::USER_STACK_TOP && end > mmu::USER_CODE_VA {
            uart::write_fmt(format_args!(
                "video: buffer 0x{start:x}+{size:x} conflita com a janela EL0\r\n"
            ));
        } else {
            uart::write_fmt(format_args!(
                "video: buffer 0x{start:x}+{size:x} nao mapeavel\r\n"
            ));
        }
        return None;
    }
    // O buffer vem do firmware sem conteúdo e pode estar em memória que a
    // MMU mapeou agora. Escreve e lê os quatro cantos antes de publicar: se
    // algum acesso falhar, o modo é recusado em vez de deixar a tela
    // apontando para memória inválida.
    if !probe(start, size, pitch, width, height) {
        return None;
    }
    unsafe {
        write_volatile(addr_of_mut!(BUFFER), start);
        write_volatile(addr_of_mut!(BUFFER_SIZE), size);
        write_volatile(addr_of_mut!(PITCH), pitch);
        write_volatile(addr_of_mut!(WIDTH), width);
        write_volatile(addr_of_mut!(HEIGHT), height);
        write_volatile(addr_of_mut!(BUFFER_BUS), bus);
        write_volatile(addr_of_mut!(AVAILABLE), true);
    }
    active()
}

/// Escreve e lê um pixel em cada canto e no centro do buffer.
fn probe(start: u64, size: usize, pitch: u32, width: u32, height: u32) -> bool {
    let stride = pitch as usize;
    let last_row = (height as usize - 1) * stride;
    let last_col = (width as usize - 1) * 2;
    let offsets = [
        0,
        last_col,
        last_row,
        last_row + last_col,
        (height as usize / 2) * stride + (width as usize / 2) * 2,
    ];
    for offset in offsets {
        let within = offset
            .checked_add(2)
            .map(|end| end <= size)
            .unwrap_or(false);
        if !within {
            return false;
        }
        let address = (start as usize + offset) as *mut u16;
        unsafe {
            write_volatile(address, 0xA5A5);
            if read_volatile(address) != 0xA5A5 {
                return false;
            }
            write_volatile(address, 0x0000);
        }
    }
    true
}

pub fn available() -> bool {
    unsafe { read_volatile(addr_of!(AVAILABLE)) }
}

/// Preenche a área visível inteira com uma cor RGB565.
pub fn clear(color: u16) {
    let (width, height) = dimensions();
    rect(0, 0, width, height, color);
}

/// Desenha um retângulo RGB565, recortando-o aos limites da tela.
pub fn rect(x: i32, y: i32, width: i32, height: i32, color: u16) {
    let Some((buffer, pitch, _, _)) = framebuffer() else {
        return;
    };
    let (screen_width, screen_height) = dimensions();
    let Some(x1) = x.checked_add(width) else {
        return;
    };
    let Some(y1) = y.checked_add(height) else {
        return;
    };
    let x0 = x.max(0).min(screen_width);
    let y0 = y.max(0).min(screen_height);
    let x1 = x1.max(0).min(screen_width);
    let y1 = y1.max(0).min(screen_height);
    if x0 >= x1 || y0 >= y1 {
        return;
    }

    for row in y0 as usize..y1 as usize {
        for column in x0 as usize..x1 as usize {
            let offset = row * pitch + column * size_of::<u16>();
            unsafe {
                write_volatile((buffer + offset) as *mut u16, color);
            }
        }
    }
}

/// Desenha texto com glifos monocromáticos 5×7 e avanço de seis pixels.
pub fn text(x: i32, y: i32, text: &str, color: u16, background: u16) {
    let Some((buffer, pitch, _, _)) = framebuffer() else {
        return;
    };
    let (screen_width, screen_height) = dimensions();
    let mut cursor_x = x;
    let mut cursor_y = y;
    for character in text.chars() {
        match character {
            '\r' => cursor_x = x,
            '\n' => {
                cursor_x = x;
                let Some(next_y) = cursor_y.checked_add(8) else {
                    break;
                };
                cursor_y = next_y;
                if cursor_y >= screen_height {
                    break;
                }
            }
            character => {
                let glyph = glyph(character);
                for (row, bits) in glyph.iter().copied().enumerate() {
                    let Some(pixel_y) = cursor_y.checked_add(row as i32) else {
                        continue;
                    };
                    if pixel_y < 0 || pixel_y >= screen_height {
                        continue;
                    }
                    for column in 0..5 {
                        let Some(pixel_x) = cursor_x.checked_add(column) else {
                            continue;
                        };
                        if pixel_x < 0 || pixel_x >= screen_width {
                            continue;
                        }
                        let pixel = if bits & (1 << (4 - column)) != 0 {
                            color
                        } else {
                            background
                        };
                        let offset = pixel_y as usize * pitch + pixel_x as usize * size_of::<u16>();
                        unsafe {
                            write_volatile((buffer + offset) as *mut u16, pixel);
                        }
                    }
                }
                let Some(next_x) = cursor_x.checked_add(6) else {
                    break;
                };
                cursor_x = next_x;
            }
        }
    }
}

pub fn exercise() -> bool {
    let Some((_, _, width, height)) = framebuffer() else {
        return false;
    };
    if width == 0 || height == 0 {
        return false;
    }
    clear(NAVY);
    // A cena acompanha a resolução atual: a faixa do título usa a largura
    // inteira e a grade de retângulos fica centrada, sem coordenadas fixas.
    rect(0, 0, width as i32, 48, BLUE);
    let margin = 32;
    let box_width = (width as i32 - margin * 2 - 96) / 3;
    let box_top = height as i32 / 4;
    let box_height = height as i32 / 4;
    for (index, color) in [RED, GREEN, WHITE].into_iter().enumerate() {
        let x = margin + index as i32 * (box_width + 48);
        rect(x, box_top, box_width, box_height, color);
    }
    let mut title = [0u8; 24];
    let title = crate::version::title(&mut title);
    text(24, 16, title, WHITE, BLUE);
    let mut line = [0u8; 40];
    let mut len = copy_text(&mut line, "framebuffer: ");
    len += decimal(&mut line[len..], width);
    line[len] = b'x';
    len += 1;
    len += decimal(&mut line[len..], height);
    len += copy_text(&mut line[len..], " rgb565");
    draw_text_bytes(48, height as i32 * 5 / 8, &line[..len], WHITE, NAVY);
    text(48, height as i32 * 5 / 8 + 24, "UART continua ativa", WHITE, NAVY);
    flush()
}

fn copy_text(destination: &mut [u8], source: &str) -> usize {
    let bytes = source.as_bytes();
    let length = destination.len().min(bytes.len());
    destination[..length].copy_from_slice(&bytes[..length]);
    length
}

fn decimal(destination: &mut [u8], value: u32) -> usize {
    let mut digits = [0u8; 10];
    let mut count = 0;
    let mut left = value;
    loop {
        digits[count] = b'0' + (left % 10) as u8;
        count += 1;
        left /= 10;
        if left == 0 {
            break;
        }
    }
    let used = count.min(destination.len());
    for index in 0..used {
        destination[index] = digits[count - 1 - index];
    }
    used
}

fn draw_text_bytes(x: i32, y: i32, bytes: &[u8], color: u16, background: u16) {
    if let Ok(value) = core::str::from_utf8(bytes) {
        text(x, y, value, color, background);
    }
}

fn framebuffer() -> Option<(usize, usize, u32, u32)> {
    unsafe {
        if !read_volatile(addr_of!(AVAILABLE)) {
            return None;
        }
        Some((
            read_volatile(addr_of!(BUFFER)) as usize,
            read_volatile(addr_of!(PITCH)) as usize,
            read_volatile(addr_of!(WIDTH)),
            read_volatile(addr_of!(HEIGHT)),
        ))
    }
}

pub(crate) fn dimensions() -> (i32, i32) {
    unsafe {
        (
            read_volatile(addr_of!(WIDTH)) as i32,
            read_volatile(addr_of!(HEIGHT)) as i32,
        )
    }
}

fn glyph(character: char) -> [u8; 7] {
    if character.is_ascii_lowercase() {
        return match character {
            'a' => [0, 0, 0b01110, 0b00001, 0b01111, 0b10001, 0b01111],
            'b' => [0b10000, 0b10000, 0b10110, 0b11001, 0b10001, 0b10001, 0b11110],
            'c' => [0, 0, 0b01111, 0b10000, 0b10000, 0b10000, 0b01111],
            'd' => [0b00001, 0b00001, 0b01101, 0b10011, 0b10001, 0b10001, 0b01111],
            'e' => [0, 0, 0b01110, 0b10001, 0b11111, 0b10000, 0b01111],
            'f' => [0b00110, 0b01001, 0b01000, 0b11100, 0b01000, 0b01000, 0b01000],
            'g' => [0, 0, 0b01111, 0b10001, 0b01111, 0b00001, 0b01110],
            'h' => [0b10000, 0b10000, 0b10110, 0b11001, 0b10001, 0b10001, 0b10001],
            'i' => [0b00100, 0, 0b01100, 0b00100, 0b00100, 0b00100, 0b01110],
            'j' => [0b00010, 0, 0b00110, 0b00010, 0b00010, 0b10010, 0b01100],
            'k' => [0b10000, 0b10000, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010],
            'l' => [0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
            'm' => [0, 0, 0b11010, 0b10101, 0b10101, 0b10101, 0b10101],
            'n' => [0, 0, 0b10110, 0b11001, 0b10001, 0b10001, 0b10001],
            'o' => [0, 0, 0b01110, 0b10001, 0b10001, 0b10001, 0b01110],
            'p' => [0, 0, 0b11110, 0b10001, 0b11110, 0b10000, 0b10000],
            'q' => [0, 0, 0b01111, 0b10001, 0b01111, 0b00001, 0b00001],
            'r' => [0, 0, 0b10111, 0b11000, 0b10000, 0b10000, 0b10000],
            's' => [0, 0, 0b01111, 0b10000, 0b01110, 0b00001, 0b11110],
            't' => [0b01000, 0b01000, 0b11100, 0b01000, 0b01000, 0b01001, 0b00110],
            'u' => [0, 0, 0b10001, 0b10001, 0b10001, 0b10011, 0b01101],
            'v' => [0, 0, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100],
            'w' => [0, 0, 0b10001, 0b10101, 0b10101, 0b10101, 0b01010],
            'x' => [0, 0, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001],
            'y' => [0, 0, 0b10001, 0b10001, 0b01111, 0b00001, 0b01110],
            'z' => [0, 0, 0b11111, 0b00010, 0b00100, 0b01000, 0b11111],
            _ => unreachable!(),
        };
    }
    let uppercase = character.to_ascii_uppercase();
    match uppercase {
        'A' => [
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'B' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ],
        'C' => [
            0b01111, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b01111,
        ],
        'D' => [
            0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
        ],
        'E' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
        'F' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'G' => [
            0b01111, 0b10000, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111,
        ],
        'H' => [
            0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'I' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b11111,
        ],
        'J' => [
            0b00111, 0b00010, 0b00010, 0b00010, 0b10010, 0b10010, 0b01100,
        ],
        'K' => [
            0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
        ],
        'L' => [
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ],
        'M' => [
            0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
        ],
        'N' => [
            0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001,
        ],
        'O' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'P' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'Q' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101,
        ],
        'R' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ],
        'S' => [
            0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        'T' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'U' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'V' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
        ],
        'W' => [
            0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001,
        ],
        'X' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
        ],
        'Y' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'Z' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
        ],
        '0' => [
            0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
        '1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        '2' => [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
        ],
        '3' => [
            0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        '4' => [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
        '5' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b00001, 0b00001, 0b11110,
        ],
        '6' => [
            0b01110, 0b10000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
        '7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        '8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        '9' => [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b01110,
        ],
        ' ' => [0; 7],
        ':' => [
            0b00000, 0b00100, 0b00100, 0b00000, 0b00100, 0b00100, 0b00000,
        ],
        '-' => [
            0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000,
        ],
        '/' => [
            0b00001, 0b00010, 0b00010, 0b00100, 0b01000, 0b01000, 0b10000,
        ],
        '.' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00110, 0b00110,
        ],
        _ => [
            0b11111, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11111,
        ],
    }
}

fn negotiate(width_requested: u32, height_requested: u32) -> Option<Negotiated> {
    // O firmware só aloca um novo framebuffer se o anterior estiver
    // liberado. Sem este passo, qualquer segunda negociação é recusada.
    release_framebuffer();
    unsafe {
        let words = addr_of_mut!(MESSAGE.words) as *mut u32;
        for index in 0..35 {
            write_volatile(words.add(index), 0);
        }
        write_volatile(words.add(0), 35 * 4);
        write_volatile(words.add(2), TAG_PHYSICAL_SIZE);
        write_volatile(words.add(3), 8);
        write_volatile(words.add(4), 8);
        write_volatile(words.add(5), width_requested);
        write_volatile(words.add(6), height_requested);
        write_volatile(words.add(7), TAG_VIRTUAL_SIZE);
        write_volatile(words.add(8), 8);
        write_volatile(words.add(9), 8);
        write_volatile(words.add(10), width_requested);
        write_volatile(words.add(11), height_requested);
        write_volatile(words.add(12), TAG_DEPTH);
        write_volatile(words.add(13), 4);
        write_volatile(words.add(14), 4);
        write_volatile(words.add(15), 16);
        write_volatile(words.add(16), TAG_ALLOCATE_BUFFER);
        write_volatile(words.add(17), 8);
        write_volatile(words.add(18), 8);
        write_volatile(words.add(19), 0);
        write_volatile(words.add(20), 0);
        write_volatile(words.add(21), TAG_PITCH);
        write_volatile(words.add(22), 4);
        write_volatile(words.add(23), 0);
        write_volatile(words.add(24), 0);
        write_volatile(words.add(25), 0);
    }
    dsb();
    cache_clean_message();

    if !wait(|status| status & MBOX_FULL == 0) {
        return None;
    }
    let message = addr_of!(MESSAGE) as usize;
    let bus = ((message as u32) | BUS_ALIAS) & !0xf;
    unsafe { write_volatile(MBOX_WRITE as *mut u32, bus | MBOX_CHANNEL) };
    dsb();

    if !wait(|status| status & MBOX_EMPTY == 0) {
        return None;
    }
    let reply = unsafe { read_volatile(MBOX_READ as *const u32) };
    dsb();
    if reply & 0xf != MBOX_CHANNEL {
        return None;
    }
    cache_invalidate_message();

    unsafe {
        let words = addr_of!(MESSAGE.words) as *const u32;
        let response_codes = [4usize, 9, 14, 18, 23];
        for index in response_codes {
            if read_volatile(words.add(index)) & (1 << 31) == 0 {
                return None;
            }
        }
        let width = read_volatile(words.add(5));
        let height = read_volatile(words.add(6));
        let virtual_width = read_volatile(words.add(10));
        let virtual_height = read_volatile(words.add(11));
        let depth = read_volatile(words.add(15));
        let buffer_bus = read_volatile(words.add(19));
        let buffer_size = read_volatile(words.add(20)) as usize;
        let pitch = read_volatile(words.add(24));
        if width != width_requested
            || height != height_requested
            || virtual_width != width_requested
            || virtual_height != height_requested
            || depth != 16
            || pitch < width * 2
            || buffer_bus & 0xf != 0
        {
            return None;
        }
        let required = (pitch as usize).checked_mul(height as usize)?;
        if buffer_size < required {
            return None;
        }
        if buffer_size & (CACHE_LINE - 1) != 0 {
            return None;
        }
        let buffer_start = if buffer_bus & BUS_ALIAS == BUS_ALIAS {
            u64::from(buffer_bus - BUS_ALIAS)
        } else {
            u64::from(buffer_bus)
        };
        if buffer_start & (CACHE_LINE as u64 - 1) != 0 {
            return None;
        }
        let buffer_end = buffer_start.checked_add(buffer_size as u64)?;
        let in_qemu_aperture =
            buffer_start >= RAM_END && buffer_end <= QEMU_FRAMEBUFFER_END;
        let in_arm_ram = buffer_end <= RAM_END;
        if !in_qemu_aperture && !in_arm_ram {
            return None;
        }
        Some(Negotiated {
            start: buffer_start,
            size: buffer_size,
            pitch,
            width,
            height,
            bus: buffer_bus,
        })
    }
}

/// Libera o framebuffer atual no firmware, se houver um publicado.
///
/// A tag `RELEASE_FRAMEBUFFER` (`0x0004_8001`) é escrita, não leitura: envia
/// o handle em uso e devolve o mesmo valor. É pré-requisito para alocar um
/// buffer de tamanho diferente.
fn release_framebuffer() {
    let handle = unsafe { read_volatile(addr_of!(BUFFER_BUS)) };
    if handle == 0 {
        return;
    }
    unsafe {
        let words = addr_of_mut!(MESSAGE.words) as *mut u32;
        for index in 0..8 {
            write_volatile(words.add(index), 0);
        }
        write_volatile(words, 8 * 4);
        write_volatile(words.add(1), 0);
        write_volatile(words.add(2), TAG_RELEASE_BUFFER);
        write_volatile(words.add(3), 4);
        write_volatile(words.add(4), 4);
        write_volatile(words.add(5), handle);
    }
    dsb();
    cache_clean_message();
    if !wait(|status| status & MBOX_FULL == 0) {
        return;
    }
    let message = addr_of!(MESSAGE) as usize;
    let bus = ((message as u32) | BUS_ALIAS) & !0xf;
    unsafe { write_volatile(MBOX_WRITE as *mut u32, bus | MBOX_CHANNEL) };
    dsb();
    if !wait(|status| status & MBOX_EMPTY == 0) {
        return;
    }
    let _ = unsafe { read_volatile(MBOX_READ as *const u32) };
    dsb();
    cache_invalidate_message();
    unsafe { write_volatile(addr_of_mut!(BUFFER_BUS), 0) };
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
    cache_maintain_message(false);
}

fn cache_invalidate_message() {
    cache_maintain_message(true);
}

/// Limpa o framebuffer normal cacheável antes de devolver o controle ao
/// console, para que o VideoCore veja todos os pixels da cena.
pub fn flush() -> bool {
    let Some((buffer, _, _, _)) = framebuffer() else {
        return false;
    };
    let size = unsafe { read_volatile(addr_of!(BUFFER_SIZE)) };
    if size == 0 {
        return false;
    }
    let Some(end) = buffer.checked_add(size) else {
        return false;
    };
    let start = buffer & !(CACHE_LINE - 1);
    let Some(aligned_end) = end.checked_add(CACHE_LINE - 1) else {
        return false;
    };
    let end = aligned_end & !(CACHE_LINE - 1);
    let mut line = start;
    while line < end {
        unsafe {
            core::arch::asm!("dc cvac, {line}", line = in(reg) line, options(nostack));
        }
        let Some(next) = line.checked_add(CACHE_LINE) else {
            return false;
        };
        line = next;
    }
    unsafe {
        core::arch::asm!("dsb sy", options(nostack, preserves_flags));
    }
    true
}

fn cache_maintain_message(invalidate: bool) {
    let message_start = addr_of!(MESSAGE) as usize;
    let Some(message_end) = message_start.checked_add(size_of::<FramebufferMessage>()) else {
        return;
    };
    let Some(aligned_end) = message_end.checked_add(CACHE_LINE - 1) else {
        return;
    };
    let start = message_start & !(CACHE_LINE - 1);
    let end = aligned_end & !(CACHE_LINE - 1);
    let mut line = start;
    while line < end {
        unsafe {
            if invalidate {
                core::arch::asm!("dc ivac, {line}", line = in(reg) line, options(nostack));
            } else {
                core::arch::asm!("dc civac, {line}", line = in(reg) line, options(nostack));
            }
        }
        let Some(next) = line.checked_add(CACHE_LINE) else {
            break;
        };
        line = next;
    }
    dsb();
}
