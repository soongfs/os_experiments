pub const SYS_WRITE: usize = 0;
pub const SYS_TIME_US: usize = 1;
pub const SYS_TXN_WRITE: usize = 2;
pub const SYS_READ_FILE: usize = 3;
pub const SYS_STAT: usize = 4;
pub const SYS_JOURNAL_DIAG: usize = 5;
pub const SYS_BOOT_MODE: usize = 6;
pub const SYS_SHUTDOWN: usize = 7;

pub const EFAULT: isize = -14;
pub const EINVAL: isize = -22;
pub const ENOENT: isize = -2;
#[allow(dead_code)]
pub const EEXIST: isize = -17;
#[allow(dead_code)]
pub const ENOTDIR: isize = -20;
#[allow(dead_code)]
pub const EISDIR: isize = -21;
#[allow(dead_code)]
pub const ENOSPC: isize = -28;
pub const ENAMETOOLONG: isize = -36;
#[allow(dead_code)]
pub const ENOTEMPTY: isize = -39;
#[allow(dead_code)]
pub const EFBIG: isize = -27;
pub const ENOSYS: isize = -38;
pub const EIO: isize = -5;

pub const FS_KIND_NONE: u8 = 0;
pub const FS_KIND_FILE: u8 = 1;
pub const FS_KIND_DIR: u8 = 2;

pub const BOOT_MODE_VERIFY: u64 = 0;
pub const BOOT_MODE_RESET_AND_CRASH: u64 = 1;
pub const BOOT_MODE_RECOVER: u64 = 2;

pub const FILE_PAYLOAD_BYTES: usize = 128;
pub const FILE_NAME_MAX: usize = 16;
pub const PATH_MAX: usize = 64;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct FsStat {
    pub kind: u8,
    pub _reserved: [u8; 7],
    pub size_bytes: u64,
    pub checksum: u64,
    pub child_count: u64,
    pub journal_active: u64,
    pub journal_committed: u64,
}

impl FsStat {
    pub const fn empty() -> Self {
        Self {
            kind: FS_KIND_NONE,
            _reserved: [0; 7],
            size_bytes: 0,
            checksum: 0,
            child_count: 0,
            journal_active: 0,
            journal_committed: 0,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct JournalDiagnostics {
    pub tx_begins: u64,
    pub tx_commits: u64,
    pub log_writes: u64,
    pub commit_writes: u64,
    pub home_writes: u64,
    pub recovery_replays: u64,
    pub crash_injections: u64,
    pub fsck_passes: u64,
    pub committed_logs_seen: u64,
    pub last_tx_seq: u64,
    pub last_home_checksum: u64,
    pub last_error: i64,
}

impl JournalDiagnostics {
    pub const fn empty() -> Self {
        Self {
            tx_begins: 0,
            tx_commits: 0,
            log_writes: 0,
            commit_writes: 0,
            home_writes: 0,
            recovery_replays: 0,
            crash_injections: 0,
            fsck_passes: 0,
            committed_logs_seen: 0,
            last_tx_seq: 0,
            last_home_checksum: 0,
            last_error: 0,
        }
    }
}
