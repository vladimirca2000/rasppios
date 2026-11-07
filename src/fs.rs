//! Sistema de arquivos mínimo no cartão SD.
//!
//! A região começa no bloco 64, longe do bloco 0 e do bloco 1 que `bloco`
//! usa. O superbloco traz a marca `RPFS` e a versão 1. O bloco 65 é a raiz,
//! o 66 é o mapa de blocos livres e os seguintes guardam arquivos e
//! diretórios. Cada arquivo cabe num bloco de 512 bytes.
//!
//! Se a marca não está lá, o comando formata só essa região. No próximo
//! boot a marca continua e o conteúdo é lido de novo.

use crate::sd;
use crate::uart;

const SB: u32 = 64;
const ROOT: u32 = 65;
const BITMAP: u32 = 66;
const DATA: u32 = 67;
const DATA_BLOCKS: u32 = 16;
const ENTRY: usize = 32;
const SLOTS: usize = 512 / ENTRY;
const FILE: u8 = 1;
const DIR: u8 = 2;

pub enum Place {
    Wrote,
    Disk,
}

/// Garante o `RPFS`. Grava `nome` só se ele ainda não existe.
pub fn place(name: &str, bytes: &[u8]) -> Option<Place> {
    if !sd::open() {
        return None;
    }
    if !mounted() && !format() {
        return None;
    }
    if lookup(ROOT, name).is_some() {
        return Some(Place::Disk);
    }
    if create_file(ROOT, name, bytes) {
        Some(Place::Wrote)
    } else {
        None
    }
}

/// Lê um arquivo da raiz para `dest`. Devolve o tamanho.
pub fn read_root(name: &str, dest: &mut [u8]) -> Option<usize> {
    let (block, size, kind) = lookup(ROOT, name)?;
    if kind != FILE || size > dest.len() || size > 512 {
        return None;
    }
    let mut data = [0u8; 512];
    if !sd::read_block(block, &mut data) {
        return None;
    }
    dest[..size].copy_from_slice(&data[..size]);
    Some(size)
}

pub fn shell_ls(path: &str) {
    if !ready() {
        uart::puts("ls: sem sistema\r\n");
        return;
    }
    let dir = if path.is_empty() {
        Some(ROOT)
    } else if path.contains('/') {
        None
    } else {
        match lookup(ROOT, path) {
            Some((block, _, DIR)) => Some(block),
            Some(_) => {
                uart::write_fmt(format_args!("ls: {path}\r\n"));
                return;
            }
            None => None,
        }
    };
    let Some(dir) = dir else {
        uart::puts("ls: ausente\r\n");
        return;
    };
    if !list_as(dir, "ls") {
        uart::puts("ls: falhou\r\n");
    }
}

pub fn shell_cat(path: &str) {
    if path.is_empty() {
        uart::puts("cat: ausente\r\n");
        return;
    }
    if !ready() {
        uart::puts("cat: sem sistema\r\n");
        return;
    }
    let Some((dir, name)) = parent_and_name(path) else {
        uart::puts("cat: ausente\r\n");
        return;
    };
    if !read_file(dir, name, "cat") {
        uart::puts("cat: ausente\r\n");
    }
}

fn ready() -> bool {
    sd::open() && mounted()
}

fn parent_and_name(path: &str) -> Option<(u32, &str)> {
    if let Some((dir, name)) = path.split_once('/') {
        if dir.is_empty() || name.is_empty() || name.contains('/') {
            return None;
        }
        let (block, _, kind) = lookup(ROOT, dir)?;
        if kind != DIR {
            return None;
        }
        Some((block, name))
    } else {
        Some((ROOT, path))
    }
}

pub fn exercise() {
    if !sd::open() {
        uart::puts("arquivos: sem cartao\r\n");
        return;
    }
    let fresh = !mounted();
    if fresh && !format() {
        uart::puts("arquivos: falhou\r\n");
        return;
    }
    // `roda` pode ter formatado o cartão só com o `prog`. Completa `ola` e
    // `notas` sem apagar o que já existe. `persistiu` só vale se já estavam lá.
    let already = demo_ready();
    if !ensure_demo() {
        uart::puts("arquivos: falhou\r\n");
        return;
    }
    if fresh {
        uart::puts("montagem: nova\r\n");
    } else {
        uart::puts("montagem: pronta\r\n");
    }
    if !list(ROOT) || !read_file(ROOT, "ola", "leitura") {
        uart::puts("arquivos: falhou\r\n");
        return;
    }
    let Some(notes) = lookup(ROOT, "notas") else {
        uart::puts("arquivos: falhou\r\n");
        return;
    };
    if notes.2 != DIR || !read_file(notes.0, "item", "sub") {
        uart::puts("arquivos: falhou\r\n");
        return;
    }
    if already {
        uart::puts("persistiu: ok\r\n");
    }
}

fn demo_ready() -> bool {
    let Some(notes) = lookup(ROOT, "notas") else {
        return false;
    };
    notes.2 == DIR && lookup(ROOT, "ola").is_some() && lookup(notes.0, "item").is_some()
}

fn ensure_demo() -> bool {
    if lookup(ROOT, "ola").is_none() && !create_file(ROOT, "ola", b"rasppios") {
        return false;
    }
    if lookup(ROOT, "notas").is_none() && !create_dir(ROOT, "notas") {
        return false;
    }
    let Some(notes) = lookup(ROOT, "notas") else {
        return false;
    };
    if notes.2 != DIR {
        return false;
    }
    lookup(notes.0, "item").is_some() || create_file(notes.0, "item", b"dentro")
}

fn mounted() -> bool {
    let mut block = [0u8; 512];
    if !sd::read_block(SB, &mut block) {
        return false;
    }
    block[0..4] == *b"RPFS" && u32_at(&block, 4) == 1
}

fn format() -> bool {
    if !zero(ROOT) || !zero(BITMAP) {
        return false;
    }
    let mut block = [0u8; 512];
    block[0..4].copy_from_slice(b"RPFS");
    block[4..8].copy_from_slice(&1u32.to_le_bytes());
    sd::write_block(SB, &block)
}

fn create_dir(parent: u32, name: &str) -> bool {
    let Some(block) = alloc() else {
        return false;
    };
    add(parent, name, DIR, block, 0)
}

fn create_file(parent: u32, name: &str, text: &[u8]) -> bool {
    if text.len() > 512 {
        return false;
    }
    let Some(block) = alloc() else {
        return false;
    };
    let mut data = [0u8; 512];
    data[..text.len()].copy_from_slice(text);
    if !sd::write_block(block, &data) {
        return false;
    }
    add(parent, name, FILE, block, text.len() as u32)
}

fn list(dir: u32) -> bool {
    list_as(dir, "lista")
}

fn list_as(dir: u32, label: &str) -> bool {
    let mut block = [0u8; 512];
    if !sd::read_block(dir, &mut block) {
        return false;
    }
    uart::puts(label);
    uart::putc(b':');
    for slot in 0..SLOTS {
        let entry = &block[slot * ENTRY..slot * ENTRY + ENTRY];
        if entry[16] == 0 {
            continue;
        }
        uart::putc(b' ');
        let end = entry[..16].iter().position(|byte| *byte == 0).unwrap_or(15);
        if let Ok(name) = core::str::from_utf8(&entry[..end]) {
            uart::puts(name);
        }
    }
    uart::puts("\r\n");
    true
}

fn read_file(dir: u32, name: &str, label: &str) -> bool {
    let Some(found) = lookup(dir, name) else {
        return false;
    };
    if found.2 != FILE || found.1 > 512 {
        return false;
    }
    let mut block = [0u8; 512];
    if !sd::read_block(found.0, &mut block) {
        return false;
    }
    let Ok(text) = core::str::from_utf8(&block[..found.1]) else {
        return false;
    };
    uart::write_fmt(format_args!("{label}: {text}\r\n"));
    true
}

/// Devolve `(bloco de dados, tamanho, tipo)`.
fn lookup(dir: u32, name: &str) -> Option<(u32, usize, u8)> {
    let mut block = [0u8; 512];
    if !sd::read_block(dir, &mut block) {
        return None;
    }
    for slot in 0..SLOTS {
        let entry = &block[slot * ENTRY..slot * ENTRY + ENTRY];
        if entry[16] == 0 || !name_eq(entry, name) {
            continue;
        }
        return Some((u32_at(entry, 20), u32_at(entry, 24) as usize, entry[16]));
    }
    None
}

fn add(dir: u32, name: &str, kind: u8, start: u32, size: u32) -> bool {
    if name.len() > 15 {
        return false;
    }
    let mut block = [0u8; 512];
    if !sd::read_block(dir, &mut block) {
        return false;
    }
    for slot in 0..SLOTS {
        let at = slot * ENTRY;
        if block[at + 16] != 0 {
            continue;
        }
        block[at..at + 16].fill(0);
        block[at..at + name.len()].copy_from_slice(name.as_bytes());
        block[at + 16] = kind;
        block[at + 20..at + 24].copy_from_slice(&start.to_le_bytes());
        block[at + 24..at + 28].copy_from_slice(&size.to_le_bytes());
        return sd::write_block(dir, &block);
    }
    false
}

fn alloc() -> Option<u32> {
    let mut map = [0u8; 512];
    if !sd::read_block(BITMAP, &mut map) {
        return None;
    }
    for index in 0..DATA_BLOCKS {
        let byte = (index / 8) as usize;
        let bit = 1u8 << (index % 8);
        if map[byte] & bit != 0 {
            continue;
        }
        map[byte] |= bit;
        if !sd::write_block(BITMAP, &map) {
            return None;
        }
        let block = DATA + index;
        if !zero(block) {
            return None;
        }
        return Some(block);
    }
    None
}

fn zero(block: u32) -> bool {
    sd::write_block(block, &[0u8; 512])
}

fn name_eq(entry: &[u8], name: &str) -> bool {
    let bytes = name.as_bytes();
    bytes.len() <= 15 && entry[..bytes.len()] == *bytes && entry[bytes.len()] == 0
}

fn u32_at(buf: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([buf[at], buf[at + 1], buf[at + 2], buf[at + 3]])
}
