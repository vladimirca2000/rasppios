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
use crate::{limits, log};

const SB: u32 = 64;
const ROOT: u32 = 65;
const BITMAP: u32 = 66;
const DATA: u32 = 67;
const DATA_BLOCKS: u32 = 16;
const INTEGRITY: u32 = 63;
const V2_SUPER_A: u32 = 61;
const V2_SUPER_B: u32 = 62;
const JOURNAL: u32 = 60;
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
    if !limits::valid_name(name) || !limits::valid_file_size(bytes.len()) {
        log::warn("rpfs-limit");
        return None;
    }
    if !sd::open() {
        log::error("sd-open");
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

pub fn check() {
    if !ready() {
        uart::puts("fsck: sem sistema\r\n");
        return;
    }
    let valid = check_dir(ROOT) && bitmap_matches();
    if !valid {
        uart::puts("fsck: corrompido\r\n");
        log::error("rpfs-check-fail");
    } else if integrity_present() && !integrity_matches() {
        uart::puts("fsck: corrompido\r\n");
        log::error("rpfs-checksum-fail");
    } else if !integrity_present() {
        if update_integrity() {
            let recovered = publish_v2_superblocks(1);
            if recovered {
                uart::puts("fsck: recuperado\r\n");
                log::info("rpfs-integrity-created");
            } else {
                uart::puts("fsck: corrompido\r\n");
                log::error("rpfs-integrity-fail");
            }
        } else {
            uart::puts("fsck: corrompido\r\n");
            log::error("rpfs-integrity-fail");
        }
    } else if v2_superblocks_valid() < 2 {
        if publish_v2_superblocks(1) {
            uart::puts("fsck: recuperado\r\n");
            log::info("rpfs-superblock-recovered");
        } else {
            uart::puts("fsck: corrompido\r\n");
            log::error("rpfs-superblock-fail");
        }
    } else {
        uart::puts("fsck: ok\r\n");
        log::info("rpfs-check-ok");
    }
}

pub fn stress() {
    if !ready() {
        uart::puts("stress: sem sistema\r\n");
        return;
    }
    let mut data = [0u8; 512];
    for _ in 0..100 {
        let Some((block, size, kind)) = lookup(ROOT, "ola") else {
            uart::puts("stress: falhou\r\n");
            return;
        };
        if kind != FILE || size > data.len() || !sd::read_block(block, &mut data) {
            uart::puts("stress: falhou\r\n");
            return;
        }
        let Some((notes, _, dir_kind)) = lookup(ROOT, "notas") else {
            uart::puts("stress: falhou\r\n");
            return;
        };
        let Some((item, item_size, item_kind)) = lookup(notes, "item") else {
            uart::puts("stress: falhou\r\n");
            return;
        };
        if dir_kind != DIR
            || item_kind != FILE
            || item_size > data.len()
            || !sd::read_block(item, &mut data)
        {
            uart::puts("stress: falhou\r\n");
            return;
        }
    }
    uart::puts("stress: ok 100\r\n");
    log::info("rpfs-stress-ok");
}

fn ready() -> bool {
    sd::open() && mounted()
}

fn check_dir(dir: u32) -> bool {
    if !limits::valid_block(dir) {
        return false;
    }
    let mut block = [0u8; 512];
    if !sd::read_block(dir, &mut block) {
        return false;
    }
    for slot in 0..SLOTS {
        let entry = &block[slot * ENTRY..slot * ENTRY + ENTRY];
        if entry[16] == 0 {
            continue;
        }
        let end = entry[..16].iter().position(|byte| *byte == 0).unwrap_or(16);
        let Ok(name) = core::str::from_utf8(&entry[..end]) else {
            return false;
        };
        if !limits::valid_name(name) || !limits::valid_block(u32_at(entry, 20)) {
            return false;
        }
        if entry[16] == FILE && !limits::valid_file_size(u32_at(entry, 24) as usize) {
            return false;
        }
        if entry[16] != FILE && entry[16] != DIR {
            return false;
        }
    }
    true
}

fn bitmap_matches() -> bool {
    let mut bitmap = [0u8; 512];
    if !sd::read_block(BITMAP, &mut bitmap) {
        return false;
    }
    for index in DATA..=82 {
        let byte = ((index - DATA) / 8) as usize;
        let bit = 1u8 << ((index - DATA) % 8);
        if bitmap[byte] & bit != 0 && !limits::valid_block(index) {
            return false;
        }
    }
    true
}

fn integrity_present() -> bool {
    let mut block = [0u8; 512];
    sd::read_block(INTEGRITY, &mut block) && block[..4] == *b"RPI1"
}

fn v2_superblocks_valid() -> u8 {
    let mut a = [0u8; 512];
    let mut b = [0u8; 512];
    let a_ok = sd::read_block(V2_SUPER_A, &mut a) && valid_v2(&a);
    let b_ok = sd::read_block(V2_SUPER_B, &mut b) && valid_v2(&b);
    (a_ok as u8) + (b_ok as u8)
}

fn valid_v2(block: &[u8; 512]) -> bool {
    block[..4] == *b"RPV2"
        && u32_at(block, 8) == ROOT
        && u32_at(block, 12) == BITMAP
        && u32_at(block, 16) == DATA
        && u32_at(block, 20) == DATA_BLOCKS
        && u32_at(block, 24) == checksum_without_field(block)
}

fn checksum_without_field(block: &[u8; 512]) -> u32 {
    let mut copy = *block;
    copy[24..28].fill(0);
    checksum(&copy)
}

fn integrity_matches() -> bool {
    let mut meta = [0u8; 512];
    let mut superblock = [0u8; 512];
    let mut root = [0u8; 512];
    let mut bitmap = [0u8; 512];
    if !sd::read_block(INTEGRITY, &mut meta)
        || !sd::read_block(SB, &mut superblock)
        || !sd::read_block(ROOT, &mut root)
        || !sd::read_block(BITMAP, &mut bitmap)
    {
        return false;
    }
    if meta[..4] != *b"RPI1"
        || u32_at(&meta, 4) != checksum(&superblock)
        || u32_at(&meta, 8) != checksum(&root)
        || u32_at(&meta, 12) != checksum(&bitmap)
    {
        return false;
    }
    for index in 0..DATA_BLOCKS {
        let byte = (index / 8) as usize;
        let bit = 1u8 << (index % 8);
        if bitmap[byte] & bit != 0 {
            let mut data = [0u8; 512];
            if !sd::read_block(DATA + index, &mut data)
                || u32_at(&meta, 16 + index as usize * 4) != checksum(&data)
            {
                return false;
            }
        }
    }
    true
}

fn update_integrity() -> bool {
    let mut meta = [0u8; 512];
    let mut superblock = [0u8; 512];
    let mut root = [0u8; 512];
    let mut bitmap = [0u8; 512];
    if !sd::read_block(SB, &mut superblock)
        || !sd::read_block(ROOT, &mut root)
        || !sd::read_block(BITMAP, &mut bitmap)
    {
        return false;
    }
    meta[..4].copy_from_slice(b"RPI1");
    meta[4..8].copy_from_slice(&checksum(&superblock).to_le_bytes());
    meta[8..12].copy_from_slice(&checksum(&root).to_le_bytes());
    meta[12..16].copy_from_slice(&checksum(&bitmap).to_le_bytes());
    for index in 0..DATA_BLOCKS {
        let byte = (index / 8) as usize;
        let bit = 1u8 << (index % 8);
        let value = if bitmap[byte] & bit != 0 {
            let mut data = [0u8; 512];
            if !sd::read_block(DATA + index, &mut data) {
                return false;
            }
            checksum(&data)
        } else {
            0
        };
        let start = 16 + index as usize * 4;
        meta[start..start + 4].copy_from_slice(&value.to_le_bytes());
    }
    let meta_sum = checksum(&meta);
    if !write_journal(1, meta_sum) {
        return false;
    }
    if !sd::write_block(INTEGRITY, &meta) {
        return false;
    }
    if !write_journal(2, meta_sum) {
        return false;
    }
    clear_journal()
}

fn checksum(block: &[u8; 512]) -> u32 {
    let mut hash = 0x811c_9dc5u32;
    for byte in block {
        hash = (hash ^ u32::from(*byte)).wrapping_mul(0x0100_0193);
    }
    hash
}

fn write_journal(state: u32, meta_sum: u32) -> bool {
    let mut block = [0u8; 512];
    block[..4].copy_from_slice(b"JRN1");
    block[4..8].copy_from_slice(&state.to_le_bytes());
    block[8..12].copy_from_slice(&meta_sum.to_le_bytes());
    let sum = checksum(&block);
    block[12..16].copy_from_slice(&sum.to_le_bytes());
    sd::write_block(JOURNAL, &block)
}

fn clear_journal() -> bool {
    sd::write_block(JOURNAL, &[0u8; 512])
}

fn recover_journal() {
    let mut block = [0u8; 512];
    if !sd::read_block(JOURNAL, &mut block) || block[..4] != *b"JRN1" {
        return;
    }
    let mut copy = block;
    copy[12..16].fill(0);
    if u32_at(&block, 12) != checksum(&copy) {
        log::error("rpfs-journal-invalid");
        return;
    }
    match u32_at(&block, 4) {
        1 => {
            let _ = clear_journal();
            log::warn("rpfs-journal-discarded");
        }
        2 => {
            let _ = clear_journal();
            log::info("rpfs-journal-committed");
        }
        _ => log::error("rpfs-journal-state"),
    }
}

fn publish_v2_superblocks(generation: u32) -> bool {
    let mut block = [0u8; 512];
    block[..4].copy_from_slice(b"RPV2");
    block[4..8].copy_from_slice(&generation.to_le_bytes());
    block[8..12].copy_from_slice(&ROOT.to_le_bytes());
    block[12..16].copy_from_slice(&BITMAP.to_le_bytes());
    block[16..20].copy_from_slice(&DATA.to_le_bytes());
    block[20..24].copy_from_slice(&DATA_BLOCKS.to_le_bytes());
    let sum = checksum(&block);
    block[24..28].copy_from_slice(&sum.to_le_bytes());
    sd::write_block(V2_SUPER_A, &block) && sd::write_block(V2_SUPER_B, &block)
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
    recover_journal();
    let mut block = [0u8; 512];
    if !sd::read_block(SB, &mut block) {
        log::warn("rpfs-read-super");
        return false;
    }
    let valid = block[0..4] == *b"RPFS" && u32_at(&block, 4) == 1;
    if valid {
        migrate_v1_metadata();
        log::info("rpfs-mount");
    } else {
        log::warn("rpfs-invalid");
    }
    valid
}

fn migrate_v1_metadata() {
    match v2_superblocks_valid() {
        2 => {}
        0 | 1 => {
            if publish_v2_superblocks(1) {
                log::info("rpfs-v1-migrated");
            } else {
                log::warn("rpfs-v1-migration-fail");
            }
        }
        _ => log::error("rpfs-v2-state"),
    }
}

fn format() -> bool {
    if !zero(ROOT) || !zero(BITMAP) {
        return false;
    }
    let mut block = [0u8; 512];
    block[0..4].copy_from_slice(b"RPFS");
    block[4..8].copy_from_slice(&1u32.to_le_bytes());
    sd::write_block(SB, &block) && update_integrity() && publish_v2_superblocks(1)
}

fn create_dir(parent: u32, name: &str) -> bool {
    let Some(block) = alloc() else {
        return false;
    };
    add(parent, name, DIR, block, 0) && update_integrity()
}

fn create_file(parent: u32, name: &str, text: &[u8]) -> bool {
    if !limits::valid_name(name) || !limits::valid_file_size(text.len()) {
        log::warn("rpfs-limit");
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
    add(parent, name, FILE, block, text.len() as u32) && update_integrity()
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
    if !limits::valid_block(dir)
        || !limits::valid_block(start)
        || !limits::valid_name(name)
        || !limits::valid_file_size(size as usize)
    {
        log::warn("rpfs-invalid-entry");
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
