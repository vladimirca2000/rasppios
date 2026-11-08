//! Contrato gráfico do rasppios: formato de pixel, geometria, superfícies e
//! mensagens da API de janelas.
//!
//! Este módulo é só contrato e validação: não desenha e não guarda buffers.
//! Ele define o que o driver de vídeo, o compositor e os clientes podem
//! combinar, para que cada lado valide o outro sem confiar em nada.
//!
//! A separação de camadas é:
//!
//! * kernel — driver de framebuffer, mapa de memória e timer;
//! * serviço gráfico — compositor, superfícies, foco e filas de eventos;
//! * aplicação — pede janela, desenha no próprio buffer e recebe eventos.
//!
//! Um cliente nunca recebe o endereço do framebuffer global. Ele recebe um
//! `Surface` com deslocamento dentro de uma arena controlada pelo serviço.

use crate::uart;

pub const BYTES_PER_PIXEL: usize = 2;
pub const MAX_WIDTH: u32 = 1920;
pub const MAX_HEIGHT: u32 = 1080;
pub const MAX_STRIDE: u32 = MAX_WIDTH * BYTES_PER_PIXEL as u32;
pub const MAX_SURFACES: usize = 8;
pub const MAX_WINDOWS: usize = 8;
pub const MAX_TITLE: usize = 32;
/// Bytes de uma mensagem da API: tipo e janela (2+2) e quatro campos de
/// 32 bits (16). O valor é conferido contra `size_of` no autoteste.
pub const MESSAGE_BYTES: usize = 20;
pub const MAX_MESSAGES: usize = 16;

/// Formato de pixel aceito pelo compositor.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    Rgb565,
}

impl PixelFormat {
    pub const fn bytes(self) -> usize {
        match self {
            PixelFormat::Rgb565 => 2,
        }
    }
}

pub const FORMAT: PixelFormat = PixelFormat::Rgb565;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Error {
    ZeroWidth,
    ZeroHeight,
    TooLarge,
    StrideTooSmall,
    StrideMisaligned,
    BufferTooSmall,
    SurfaceOutsideArena,
    UnknownMessage,
    TitleTooLong,
    WindowOutOfRange,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub const fn is_empty(&self) -> bool {
        self.width <= 0 || self.height <= 0
    }

    pub fn right(&self) -> i32 {
        self.x.saturating_add(self.width)
    }

    pub fn bottom(&self) -> i32 {
        self.y.saturating_add(self.height)
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        !self.is_empty() && x >= self.x && y >= self.y && x < self.right() && y < self.bottom()
    }

    /// Interseção de dois retângulos; vazia quando não se sobrepõem.
    pub fn intersect(&self, other: &Rect) -> Rect {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        Rect::new(x, y, right - x, bottom - y)
    }
}

/// Descrição de um modo de vídeo pronto para ser aplicado.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Mode {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub pixel_format: PixelFormat,
}

impl Mode {
    pub const fn new(width: u32, height: u32, stride: u32) -> Self {
        Self {
            width,
            height,
            stride,
            pixel_format: FORMAT,
        }
    }

    /// Bytes necessários para a imagem inteira, sem contar alinhamento.
    pub const fn buffer_len(&self) -> usize {
        self.stride as usize * self.height as usize
    }

    pub fn validate(&self) -> Result<(), Error> {
        if self.width == 0 {
            return Err(Error::ZeroWidth);
        }
        if self.height == 0 {
            return Err(Error::ZeroHeight);
        }
        if self.width > MAX_WIDTH || self.height > MAX_HEIGHT {
            return Err(Error::TooLarge);
        }
        let minimum = self.width * self.pixel_format.bytes() as u32;
        if self.stride < minimum {
            return Err(Error::StrideTooSmall);
        }
        if self.stride > MAX_STRIDE {
            return Err(Error::TooLarge);
        }
        if self.stride % self.pixel_format.bytes() as u32 != 0 {
            return Err(Error::StrideMisaligned);
        }
        Ok(())
    }
}

/// Região de desenho de um cliente dentro da arena do compositor.
///
/// `offset` e `length` são relativos à arena, não ao framebuffer. O serviço
/// confere os dois antes de publicar a superfície na tela.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Surface {
    pub id: u16,
    pub rect: Rect,
    pub stride: u32,
    pub offset: usize,
    pub length: usize,
}

impl Surface {
    pub fn validate(&self, arena_len: usize) -> Result<(), Error> {
        if self.rect.is_empty() {
            return Err(Error::ZeroWidth);
        }
        if self.rect.width as u32 > MAX_WIDTH || self.rect.height as u32 > MAX_HEIGHT {
            return Err(Error::TooLarge);
        }
        let minimum = self.rect.width as u32 * BYTES_PER_PIXEL as u32;
        if self.stride < minimum {
            return Err(Error::StrideTooSmall);
        }
        if self.stride % BYTES_PER_PIXEL as u32 != 0 {
            return Err(Error::StrideMisaligned);
        }
        let needed = self.stride as usize * self.rect.height as usize;
        if self.length < needed {
            return Err(Error::BufferTooSmall);
        }
        match self.offset.checked_add(self.length) {
            Some(end) if end <= arena_len => Ok(()),
            _ => Err(Error::SurfaceOutsideArena),
        }
    }
}

pub const MSG_CREATE: u16 = 1;
pub const MSG_DESTROY: u16 = 2;
pub const MSG_MOVE: u16 = 3;
pub const MSG_RESIZE: u16 = 4;
pub const MSG_RAISE: u16 = 5;
pub const MSG_DAMAGE: u16 = 6;
pub const MSG_TITLE: u16 = 7;
pub const MSG_CLOSE: u16 = 8;

pub const EV_KEY: u16 = 16;
pub const EV_POINTER: u16 = 17;
pub const EV_FOCUS: u16 = 18;
pub const EV_CLOSE: u16 = 19;

/// Mensagem fixa trocada entre cliente e serviço gráfico.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Message {
    pub kind: u16,
    pub window: u16,
    pub a: i32,
    pub b: i32,
    pub c: i32,
    pub d: i32,
}

impl Message {
    pub const fn new(kind: u16, window: u16, a: i32, b: i32, c: i32, d: i32) -> Self {
        Self {
            kind,
            window,
            a,
            b,
            c,
            d,
        }
    }

    pub const fn create(window: u16, x: i32, y: i32, width: i32, height: i32) -> Self {
        Message::new(MSG_CREATE, window, x, y, width, height)
    }

    pub const fn destroy(window: u16) -> Self {
        Message::new(MSG_DESTROY, window, 0, 0, 0, 0)
    }

    pub const fn move_to(window: u16, x: i32, y: i32) -> Self {
        Message::new(MSG_MOVE, window, x, y, 0, 0)
    }

    pub const fn resize(window: u16, width: i32, height: i32) -> Self {
        Message::new(MSG_RESIZE, window, width, height, 0, 0)
    }

    pub const fn title(window: u16, length: i32) -> Self {
        Message::new(MSG_TITLE, window, length, 0, 0, 0)
    }

    pub const fn damage(window: u16, rect: Rect) -> Self {
        Message::new(MSG_DAMAGE, window, rect.x, rect.y, rect.width, rect.height)
    }

    pub fn validate(&self) -> Result<(), Error> {
        if (self.window as usize) >= MAX_WINDOWS {
            return Err(Error::WindowOutOfRange);
        }
        match self.kind {
            MSG_CREATE | MSG_RESIZE => {
                // `create` guarda x,y em `a,b` e largura,altura em `c,d`;
                // `resize` guarda largura,altura em `a,b`.
                let (width, height) = if self.kind == MSG_CREATE {
                    (self.c, self.d)
                } else {
                    (self.a, self.b)
                };
                if width <= 0 || height <= 0 {
                    return Err(Error::ZeroWidth);
                }
                if width as u32 > MAX_WIDTH || height as u32 > MAX_HEIGHT {
                    return Err(Error::TooLarge);
                }
                Ok(())
            }
            MSG_MOVE | MSG_DESTROY | MSG_RAISE | MSG_DAMAGE | MSG_CLOSE => Ok(()),
            MSG_TITLE => {
                if self.a < 0 || self.a as usize > MAX_TITLE {
                    return Err(Error::TitleTooLong);
                }
                Ok(())
            }
            _ => Err(Error::UnknownMessage),
        }
    }
}

fn check(label: &str, ok: bool, failures: &mut u32) {
    if !ok {
        *failures += 1;
    }
    uart::write_fmt(format_args!(
        "gfx {label}: {}\r\n",
        if ok { "ok" } else { "falhou" }
    ));
}

/// Exercita o contrato sem tocar no framebuffer. Devolve `true` se todas as
/// verificações passaram.
pub fn self_test() -> bool {
    let mut failures = 0;

    let mode = Mode::new(640, 480, 1280);
    check("modo 640x480", mode.validate().is_ok(), &mut failures);
    check(
        "buffer do modo",
        mode.buffer_len() == 1280 * 480,
        &mut failures,
    );
    check(
        "bpp rgb565",
        FORMAT.bytes() == BYTES_PER_PIXEL && BYTES_PER_PIXEL == 2,
        &mut failures,
    );
    check(
        "largura zero",
        Mode::new(0, 480, 1280).validate() == Err(Error::ZeroWidth),
        &mut failures,
    );
    check(
        "altura zero",
        Mode::new(640, 0, 1280).validate() == Err(Error::ZeroHeight),
        &mut failures,
    );
    check(
        "modo grande demais",
        Mode::new(MAX_WIDTH + 1, 480, 4000).validate() == Err(Error::TooLarge),
        &mut failures,
    );
    check(
        "stride curto",
        Mode::new(640, 480, 1200).validate() == Err(Error::StrideTooSmall),
        &mut failures,
    );
    check(
        "stride impar",
        Mode::new(640, 480, 1281).validate() == Err(Error::StrideMisaligned),
        &mut failures,
    );

    let rect = Rect::new(10, 20, 100, 50);
    check(
        "recorte",
        rect.intersect(&Rect::new(50, 40, 100, 50)) == Rect::new(50, 40, 60, 30),
        &mut failures,
    );
    check(
        "fora do recorte",
        rect.intersect(&Rect::new(500, 500, 10, 10)).is_empty(),
        &mut failures,
    );
    check(
        "contem",
        rect.contains(10, 20) && rect.contains(109, 69) && !rect.contains(110, 70),
        &mut failures,
    );
    check(
        "retangulo vazio",
        Rect::new(5, 5, 0, 10).is_empty(),
        &mut failures,
    );

    let arena = 614_400;
    let surface = Surface {
        id: 1,
        rect: Rect::new(0, 0, 640, 480),
        stride: 1280,
        offset: 0,
        length: arena,
    };
    check(
        "superficie na arena",
        surface.validate(arena).is_ok(),
        &mut failures,
    );
    let outside = Surface {
        offset: 64,
        ..surface
    };
    check(
        "superficie fora da arena",
        outside.validate(arena) == Err(Error::SurfaceOutsideArena),
        &mut failures,
    );
    let small = Surface {
        length: 1024,
        ..surface
    };
    check(
        "superficie pequena",
        small.validate(arena) == Err(Error::BufferTooSmall),
        &mut failures,
    );

    check(
        "msg criar",
        Message::create(0, 10, 10, 200, 100).validate().is_ok(),
        &mut failures,
    );
    check(
        "msg criar sem area",
        Message::create(0, 10, 10, 0, 100).validate() == Err(Error::ZeroWidth),
        &mut failures,
    );
    check(
        "msg janela invalida",
        Message::move_to(MAX_WINDOWS as u16, 1, 1).validate() == Err(Error::WindowOutOfRange),
        &mut failures,
    );
    check(
        "msg titulo longo",
        Message::title(0, (MAX_TITLE + 1) as i32).validate() == Err(Error::TitleTooLong),
        &mut failures,
    );
    check(
        "msg desconhecida",
        Message::new(999, 0, 0, 0, 0, 0).validate() == Err(Error::UnknownMessage),
        &mut failures,
    );
    check(
        "msg destruir",
        Message::destroy(MAX_WINDOWS as u16 - 1).validate().is_ok(),
        &mut failures,
    );
    check(
        "msg redimensionar",
        Message::resize(0, 320, 200).validate().is_ok()
            && Message::resize(0, 0, 200).validate() == Err(Error::ZeroWidth),
        &mut failures,
    );
    check(
        "msg dano",
        Message::damage(0, Rect::new(1, 2, 3, 4)).validate().is_ok(),
        &mut failures,
    );

    // Eventos do serviço para o cliente: mesma largura fixa da mensagem.
    check(
        "eventos definidos",
        EV_KEY != EV_POINTER
            && EV_POINTER != EV_FOCUS
            && EV_FOCUS != EV_CLOSE
            && EV_CLOSE != MSG_CLOSE,
        &mut failures,
    );
    check(
        "mensagem 20 bytes",
        MESSAGE_BYTES == 20 && MESSAGE_BYTES == core::mem::size_of::<Message>(),
        &mut failures,
    );
    check(
        "limites de recurso",
        MAX_SURFACES >= 4 && MAX_WINDOWS >= 4 && MAX_MESSAGES >= 8,
        &mut failures,
    );

    if failures == 0 {
        uart::puts("gfx: contrato ok\r\n");
    } else {
        uart::write_fmt(format_args!("gfx: {failures} falhas\r\n"));
    }
    failures == 0
}
