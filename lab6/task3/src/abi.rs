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
pub const FS_DEVICE_ID: u64 = 0x4c36_0001;

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

#[repr(C)]
#[derive(Clone, Copy)]
pub struct FsStat {
    pub kind: u8,
    pub highest_level: u8,
    pub _reserved: [u8; 6],
    pub inode_number: u64,
    pub device_id: u64,
    pub size_bytes: u64,
    pub blocks_used: u64,
    pub child_count: u64,
    pub created_us: u64,
    pub modified_us: u64,
}

impl FsStat {
    pub const fn empty() -> Self {
        Self {
            kind: FS_KIND_NONE,
            highest_level: FS_LEVEL_DIRECT,
            _reserved: [0; 6],
            inode_number: 0,
            device_id: 0,
            size_bytes: 0,
            blocks_used: 0,
            child_count: 0,
            created_us: 0,
            modified_us: 0,
        }
    }
}
