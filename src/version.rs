//! Identidade da versão instalada e procedimento de recuperação.
//!
//! Este é o único lugar que define o número do marco. O banner da UART, o
//! título da GUI, o comando `versao` e o teste automatizado derivam daqui,
//! para que o número não possa divergir entre a tela e o console.

use core::fmt::Write;

pub const MARCO: u8 = 23;

/// Escreve `rasppios marco NN` em `buf` e devolve o texto.
///
/// A GUI usa esta função em vez de uma string literal, então mudar `MARCO`
/// atualiza a tela e o console ao mesmo tempo.
pub fn title(buf: &mut [u8]) -> &str {
    struct Out<'a> {
        buf: &'a mut [u8],
        len: usize,
    }

    impl Write for Out<'_> {
        fn write_str(&mut self, s: &str) -> core::fmt::Result {
            let bytes = s.as_bytes();
            if self.len + bytes.len() > self.buf.len() {
                return Err(core::fmt::Error);
            }
            self.buf[self.len..self.len + bytes.len()].copy_from_slice(bytes);
            self.len += bytes.len();
            Ok(())
        }
    }

    let mut out = Out { buf, len: 0 };
    let _ = write!(out, "rasppios marco {MARCO}");
    let len = out.len;
    // O buffer só recebe ASCII, então o recorte é sempre válido.
    core::str::from_utf8(&out.buf[..len]).unwrap_or("rasppios")
}

/// Resto do bloco de versão, impresso depois de `versao: rasppios marco NN`.
pub const TEXT: &str = "\
formato: kernel8.img AArch64, framebuffer 640x480 rgb565\r\n\
recuperacao: sem RPFS o boot segue e a UART responde\r\n\
             comandos: help, fsck, stress, verifica, logs\r\n\
             para restaurar os arquivos de exemplo: arquivos\r\n";

/// Imprime a identificação completa na UART.
pub fn print() {
    let mut buf = [0u8; 24];
    let title = title(&mut buf);
    crate::uart::write_fmt(format_args!("versao: {title}\r\n{}", TEXT));
}
