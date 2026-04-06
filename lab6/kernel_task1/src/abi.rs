use core::mem::size_of;

pub const SYS_WRITE: usize = 0;
pub const SYS_TIME_US: usize = 1;
pub const SYS_CREATE_DIR: usize = 2;
pub const SYS_CREATE_FILE: usize = 3;
pub const SYS_WRITE_AT: usize = 4;
pub const SYS_READ_AT: usize = 5;
pub const SYS_STAT: usize = 6;
pub const SYS_REMOVE: usize = 7;
pub const SYS_LIST_DIR: usize = 8;
pub const SYS_SHUTDOWN: usize = 9;
pub const SYS_FS_DIAG: usize = 10;

pub const EFAULT: isize = -14;
pub const EINVAL: isize = -22;
pub const ENOENT: isize = -2;
pub const EEXIST: isize = -17;
pub const ENOTDIR: isize = -20;
pub const EISDIR: isize = -21;
pub const ENOSPC: isize = -28;
pub const ENAMETOOLONG: isize = -36;
pub const ENOTEMPTY: isize = -39;
pub const EFBIG: isize = -27;
pub const ENOSYS: isize = -38;

pub const FS_KIND_NONE: u8 = 0;
pub const FS_KIND_FILE: u8 = 1;
pub const FS_KIND_DIR: u8 = 2;

pub const FS_LEVEL_DIRECT: u8 = 0;
pub const FS_LEVEL_SINGLE: u8 = 1;
pub const FS_LEVEL_DOUBLE: u8 = 2;
pub const FS_LEVEL_TRIPLE: u8 = 3;

pub const FS_BLOCK_SIZE: usize = 512;
pub const FS_DIRECT_POINTERS: usize = 10;
pub const FS_POINTERS_PER_BLOCK: usize = FS_BLOCK_SIZE / size_of::<u32>();
pub const FS_NAME_MAX: usize = 24;
pub const FS_PATH_MAX: usize = 256;
pub const FS_MAX_DIR_ENTRIES: usize = 32;

pub const FS_SINGLE_LIMIT_BLOCKS: u64 = (FS_DIRECT_POINTERS + FS_POINTERS_PER_BLOCK) as u64;
pub const FS_DOUBLE_LIMIT_BLOCKS: u64 =
    FS_SINGLE_LIMIT_BLOCKS + (FS_POINTERS_PER_BLOCK * FS_POINTERS_PER_BLOCK) as u64;
pub const FS_TRIPLE_LIMIT_BLOCKS: u64 = FS_DOUBLE_LIMIT_BLOCKS
    + (FS_POINTERS_PER_BLOCK * FS_POINTERS_PER_BLOCK * FS_POINTERS_PER_BLOCK) as u64;

pub const FS_SINGLE_LIMIT_BYTES: u64 = FS_SINGLE_LIMIT_BLOCKS * FS_BLOCK_SIZE as u64;
pub const FS_DOUBLE_LIMIT_BYTES: u64 = FS_DOUBLE_LIMIT_BLOCKS * FS_BLOCK_SIZE as u64;
pub const FS_TRIPLE_LIMIT_BYTES: u64 = FS_TRIPLE_LIMIT_BLOCKS * FS_BLOCK_SIZE as u64;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct FsStat {
    pub kind: u8,
    pub highest_level: u8,
    pub _reserved: [u8; 6],
    pub size_bytes: u64,
    pub blocks_used: u64,
    pub child_count: u64,
}

impl FsStat {
    pub const fn empty() -> Self {
        Self {
            kind: FS_KIND_NONE,
            highest_level: FS_LEVEL_DIRECT,
            _reserved: [0; 6],
            size_bytes: 0,
            blocks_used: 0,
            child_count: 0,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct FsDiagnostics {
    pub disk_inode_size: u32,
    pub direct_offset: u32,
    pub single_offset: u32,
    pub double_offset: u32,
    pub triple_offset: u32,
    pub recursive_calls: u64,
    pub max_recursion_depth: u64,
    pub triple_resolution_calls: u64,
    pub pointer_blocks_used: u64,
    pub data_blocks_used: u64,
}

impl FsDiagnostics {
    pub const fn empty() -> Self {
        Self {
            disk_inode_size: 0,
            direct_offset: 0,
            single_offset: 0,
            double_offset: 0,
            triple_offset: 0,
            recursive_calls: 0,
            max_recursion_depth: 0,
            triple_resolution_calls: 0,
            pointer_blocks_used: 0,
            data_blocks_used: 0,
        }
    }
}
