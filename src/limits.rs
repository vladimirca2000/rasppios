//! Limites comuns dos recursos expostos pelo RPFS.

pub const RPFS_FIRST: u32 = 64;
pub const RPFS_LAST: u32 = 82;
pub const MAX_NAME: usize = 15;
pub const MAX_FILE: usize = 512;

pub fn valid_block(block: u32) -> bool {
    (RPFS_FIRST..=RPFS_LAST).contains(&block)
}

pub fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= MAX_NAME && !name.contains('/')
}

pub fn valid_file_size(size: usize) -> bool {
    size <= MAX_FILE
}
