use core::mem::size_of;
use core::ptr;
use core::slice;

use crate::abi::{
    FsStat, JournalDiagnostics, BOOT_MODE_RECOVER, BOOT_MODE_RESET_AND_CRASH, BOOT_MODE_VERIFY,
    EIO, EINVAL, ENOENT, FS_KIND_DIR, FS_KIND_FILE, FILE_NAME_MAX, FILE_PAYLOAD_BYTES,
};
use crate::hostio;
use crate::println;

const DISK_MAGIC: u32 = 0x4a36_4c35;
const DISK_VERSION: u32 = 1;
const FILE_PATH: &[u8] = b"/journaled.txt";
const FILE_NAME: &[u8] = b"journaled.txt";
const INITIAL_BYTES: &[u8] = b"seed-before-crash\n";
const UPDATED_BYTES: &[u8] = b"after-crash-journal-replay\n";
const INITIAL_CHECKSUM: u64 = checksum_const(INITIAL_BYTES);

#[repr(C)]
#[derive(Clone, Copy)]
struct RootInode {
    valid: u8,
    kind: u8,
    _padding: [u8; 2],
    child_count: u32,
    size_bytes: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct DirentDisk {
    valid: u8,
    name_len: u8,
    _padding: [u8; 2],
    inode_index: u32,
    name: [u8; FILE_NAME_MAX],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct FileInode {
    valid: u8,
    kind: u8,
    _padding: [u8; 2],
    size_bytes: u32,
    checksum: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct JournalHeader {
    active: u8,
    committed: u8,
    _padding: [u8; 2],
    target_inode: u32,
    tx_seq: u64,
    logged_size: u32,
    _reserved: u32,
    logged_checksum: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct DiskImage {
    magic: u32,
    version: u32,
    root_inode: RootInode,
    root_dirent: DirentDisk,
    home_inode: FileInode,
    home_data: [u8; FILE_PAYLOAD_BYTES],
    log_header: JournalHeader,
    log_inode: FileInode,
    log_data: [u8; FILE_PAYLOAD_BYTES],
}

impl DiskImage {
    const fn empty() -> Self {
        Self {
            magic: DISK_MAGIC,
            version: DISK_VERSION,
            root_inode: RootInode {
                valid: 1,
                kind: FS_KIND_DIR,
                _padding: [0; 2],
                child_count: 1,
                size_bytes: size_of::<DirentDisk>() as u32,
            },
            root_dirent: DirentDisk {
                valid: 1,
                name_len: FILE_NAME.len() as u8,
                _padding: [0; 2],
                inode_index: 1,
                name: padded_name(FILE_NAME),
            },
            home_inode: FileInode {
                valid: 1,
                kind: FS_KIND_FILE,
                _padding: [0; 2],
                size_bytes: INITIAL_BYTES.len() as u32,
                checksum: INITIAL_CHECKSUM,
            },
            home_data: padded_data(INITIAL_BYTES),
            log_header: JournalHeader {
                active: 0,
                committed: 0,
                _padding: [0; 2],
                target_inode: 1,
                tx_seq: 0,
                logged_size: 0,
                _reserved: 0,
                logged_checksum: 0,
            },
            log_inode: FileInode {
                valid: 0,
                kind: FS_KIND_FILE,
                _padding: [0; 2],
                size_bytes: 0,
                checksum: 0,
            },
            log_data: [0; FILE_PAYLOAD_BYTES],
        }
    }
}

static mut DISK_IMAGE: DiskImage = DiskImage::empty();
static mut DIAGNOSTICS: JournalDiagnostics = JournalDiagnostics::empty();
static mut BOOT_MODE: u64 = BOOT_MODE_VERIFY;

pub fn init() {
    unsafe {
        DIAGNOSTICS = JournalDiagnostics::empty();
        BOOT_MODE = detect_boot_mode();
    }

    let reset = unsafe { BOOT_MODE == BOOT_MODE_RESET_AND_CRASH };
    if reset {
        unsafe {
            DISK_IMAGE = DiskImage::empty();
        }
        flush_disk("reset-image").unwrap_or_else(record_error);
    } else if !load_disk() {
        unsafe {
            DISK_IMAGE = DiskImage::empty();
        }
        flush_disk("create-image").unwrap_or_else(record_error);
    }

    if !valid_disk() {
        unsafe {
            DISK_IMAGE = DiskImage::empty();
        }
        flush_disk("repair-invalid-image").unwrap_or_else(record_error);
    }

    recover_if_needed();
    let _ = fsck();
}

pub fn boot_mode() -> u64 {
    unsafe { BOOT_MODE }
}

pub fn expected_updated_bytes() -> &'static [u8] {
    UPDATED_BYTES
}

pub fn file_path() -> &'static str {
    "/journaled.txt"
}

pub fn journal_diagnostics() -> JournalDiagnostics {
    unsafe { DIAGNOSTICS }
}

pub fn read_file(path: &[u8], dst: &mut [u8]) -> isize {
    if path != FILE_PATH {
        return ENOENT;
    }
    unsafe {
        let size = DISK_IMAGE.home_inode.size_bytes as usize;
        if dst.len() < size {
            return EINVAL;
        }
        dst[..size].copy_from_slice(&DISK_IMAGE.home_data[..size]);
        size as isize
    }
}

pub fn stat(path: &[u8], stat: &mut FsStat) -> isize {
    unsafe {
        if path == b"/" {
            stat.kind = FS_KIND_DIR;
            stat.size_bytes = DISK_IMAGE.root_inode.size_bytes as u64;
            stat.checksum = 0;
            stat.child_count = DISK_IMAGE.root_inode.child_count as u64;
            stat.journal_active = DISK_IMAGE.log_header.active as u64;
            stat.journal_committed = DISK_IMAGE.log_header.committed as u64;
            return 0;
        }
        if path == FILE_PATH {
            stat.kind = FS_KIND_FILE;
            stat.size_bytes = DISK_IMAGE.home_inode.size_bytes as u64;
            stat.checksum = DISK_IMAGE.home_inode.checksum;
            stat.child_count = 0;
            stat.journal_active = DISK_IMAGE.log_header.active as u64;
            stat.journal_committed = DISK_IMAGE.log_header.committed as u64;
            return 0;
        }
    }
    ENOENT
}

pub fn transactional_write(path: &[u8], bytes: &[u8]) -> isize {
    if path != FILE_PATH {
        return ENOENT;
    }
    if bytes.len() > FILE_PAYLOAD_BYTES {
        return EINVAL;
    }

    unsafe {
        DIAGNOSTICS.tx_begins += 1;
        DIAGNOSTICS.last_tx_seq = DISK_IMAGE.log_header.tx_seq + 1;
    }
    println!(
        "[journal] tx={} stage=begin target={}",
        unsafe { DIAGNOSTICS.last_tx_seq },
        file_path()
    );

    let new_inode = FileInode {
        valid: 1,
        kind: FS_KIND_FILE,
        _padding: [0; 2],
        size_bytes: bytes.len() as u32,
        checksum: checksum(bytes),
    };
    let mut new_data = [0u8; FILE_PAYLOAD_BYTES];
    new_data[..bytes.len()].copy_from_slice(bytes);

    unsafe {
        DISK_IMAGE.log_inode = new_inode;
        DISK_IMAGE.log_data = new_data;
        DISK_IMAGE.log_header.active = 1;
        DISK_IMAGE.log_header.committed = 0;
        DISK_IMAGE.log_header.target_inode = 1;
        DISK_IMAGE.log_header.tx_seq = DIAGNOSTICS.last_tx_seq;
        DISK_IMAGE.log_header.logged_size = bytes.len() as u32;
        DISK_IMAGE.log_header.logged_checksum = new_inode.checksum;
    }
    flush_disk("write-log-blocks").unwrap_or_else(record_error);
    unsafe {
        DIAGNOSTICS.log_writes += 1;
    }
    println!(
        "[journal] tx={} stage=write-log data_checksum={:#018x}",
        unsafe { DIAGNOSTICS.last_tx_seq },
        new_inode.checksum
    );

    unsafe {
        DISK_IMAGE.log_header.committed = 1;
        DIAGNOSTICS.tx_commits += 1;
        DIAGNOSTICS.commit_writes += 1;
    }
    flush_disk("write-commit").unwrap_or_else(record_error);
    println!(
        "[journal] tx={} stage=commit committed=1",
        unsafe { DIAGNOSTICS.last_tx_seq }
    );

    if unsafe { BOOT_MODE == BOOT_MODE_RESET_AND_CRASH } {
        unsafe {
            DIAGNOSTICS.crash_injections += 1;
        }
        println!(
            "[journal] tx={} stage=crash-inject reason=after-commit-before-home-install",
            unsafe { DIAGNOSTICS.last_tx_seq }
        );
        return 1;
    }

    install_logged_transaction("install-home");
    0
}

pub fn fsck() -> bool {
    let pass = unsafe {
        DISK_IMAGE.magic == DISK_MAGIC
            && DISK_IMAGE.version == DISK_VERSION
            && DISK_IMAGE.root_inode.valid == 1
            && DISK_IMAGE.root_inode.kind == FS_KIND_DIR
            && DISK_IMAGE.root_inode.child_count == 1
            && DISK_IMAGE.root_inode.size_bytes == size_of::<DirentDisk>() as u32
            && DISK_IMAGE.root_dirent.valid == 1
            && DISK_IMAGE.root_dirent.inode_index == 1
            && DISK_IMAGE.root_dirent.name_len as usize == FILE_NAME.len()
            && &DISK_IMAGE.root_dirent.name[..FILE_NAME.len()] == FILE_NAME
            && DISK_IMAGE.home_inode.valid == 1
            && DISK_IMAGE.home_inode.kind == FS_KIND_FILE
            && DISK_IMAGE.home_inode.size_bytes as usize <= FILE_PAYLOAD_BYTES
            && checksum(&DISK_IMAGE.home_data[..DISK_IMAGE.home_inode.size_bytes as usize])
                == DISK_IMAGE.home_inode.checksum
            && !(DISK_IMAGE.log_header.active == 1 && DISK_IMAGE.log_header.committed == 1)
    };

    if pass {
        unsafe {
            DIAGNOSTICS.fsck_passes += 1;
            DIAGNOSTICS.last_home_checksum = DISK_IMAGE.home_inode.checksum;
            DIAGNOSTICS.last_error = 0;
        }
    } else {
        unsafe {
            DIAGNOSTICS.last_error = EIO as i64;
        }
    }

    println!(
        "[fsck] root_dirent_ok={} home_inode_ok={} home_checksum_ok={} journal_clear={}",
        if unsafe {
            DISK_IMAGE.root_dirent.valid == 1
                && DISK_IMAGE.root_dirent.inode_index == 1
                && &DISK_IMAGE.root_dirent.name[..FILE_NAME.len()] == FILE_NAME
        } { "yes" } else { "no" },
        if unsafe { DISK_IMAGE.home_inode.valid == 1 && DISK_IMAGE.home_inode.kind == FS_KIND_FILE } {
            "yes"
        } else {
            "no"
        },
        if unsafe {
            checksum(&DISK_IMAGE.home_data[..DISK_IMAGE.home_inode.size_bytes as usize])
                == DISK_IMAGE.home_inode.checksum
        } { "yes" } else { "no" },
        if unsafe { !(DISK_IMAGE.log_header.active == 1 && DISK_IMAGE.log_header.committed == 1) } {
            "yes"
        } else {
            "no"
        }
    );
    pass
}

fn recover_if_needed() {
    unsafe {
        if DISK_IMAGE.log_header.active == 1 && DISK_IMAGE.log_header.committed == 1 {
            DIAGNOSTICS.committed_logs_seen += 1;
            let tx_seq = DISK_IMAGE.log_header.tx_seq;
            DIAGNOSTICS.last_tx_seq = tx_seq;
            println!(
                "[recovery] tx={} committed_log_detected=yes action=replay",
                tx_seq
            );
            install_logged_transaction("replay-log");
            DIAGNOSTICS.recovery_replays += 1;
        } else if DISK_IMAGE.log_header.active == 1 && DISK_IMAGE.log_header.committed == 0 {
            let tx_seq = DISK_IMAGE.log_header.tx_seq;
            println!(
                "[recovery] tx={} committed_log_detected=no action=discard",
                tx_seq
            );
            DISK_IMAGE.log_header.active = 0;
            DISK_IMAGE.log_header.committed = 0;
            flush_disk("discard-uncommitted-log").unwrap_or_else(record_error);
        }
    }
}

fn install_logged_transaction(stage: &str) {
    let tx_seq;
    let home_checksum;
    unsafe {
        DISK_IMAGE.home_inode = DISK_IMAGE.log_inode;
        DISK_IMAGE.home_data = DISK_IMAGE.log_data;
        DIAGNOSTICS.home_writes += 1;
        DIAGNOSTICS.last_home_checksum = DISK_IMAGE.home_inode.checksum;
        tx_seq = DISK_IMAGE.log_header.tx_seq;
        home_checksum = DISK_IMAGE.home_inode.checksum;
    }
    flush_disk(stage).unwrap_or_else(record_error);
    println!(
        "[journal] tx={} stage={} home_checksum={:#018x}",
        tx_seq,
        stage,
        home_checksum
    );

    unsafe {
        DISK_IMAGE.log_header.active = 0;
        DISK_IMAGE.log_header.committed = 0;
        DISK_IMAGE.log_header.logged_size = 0;
        DISK_IMAGE.log_header.logged_checksum = 0;
        DISK_IMAGE.log_inode = FileInode {
            valid: 0,
            kind: FS_KIND_FILE,
            _padding: [0; 2],
            size_bytes: 0,
            checksum: 0,
        };
        ptr::write_bytes(
            ptr::addr_of_mut!(DISK_IMAGE.log_data) as *mut u8,
            0,
            FILE_PAYLOAD_BYTES,
        );
    }
    flush_disk("clear-log").unwrap_or_else(record_error);
    let cleared_tx_seq = unsafe { DISK_IMAGE.log_header.tx_seq };
    println!(
        "[journal] tx={} stage=clear-log active=0 committed=0",
        cleared_tx_seq
    );
}

fn load_disk() -> bool {
    let buffer = unsafe {
        slice::from_raw_parts_mut(
            ptr::addr_of_mut!(DISK_IMAGE) as *mut u8,
            size_of::<DiskImage>(),
        )
    };
    match hostio::read_disk_image(buffer) {
        Ok(found) => found,
        Err(err) => {
            record_error(err);
            false
        }
    }
}

fn flush_disk(stage: &str) -> Result<(), isize> {
    let bytes = unsafe {
        slice::from_raw_parts(
            ptr::addr_of!(DISK_IMAGE) as *const u8,
            size_of::<DiskImage>(),
        )
    };
    let result = hostio::write_disk_image(bytes);
    if result.is_ok() {
        println!("[host-disk] stage={} image={} bytes={}", stage, hostio::disk_path(), bytes.len());
    }
    result
}

fn valid_disk() -> bool {
    unsafe { DISK_IMAGE.magic == DISK_MAGIC && DISK_IMAGE.version == DISK_VERSION }
}

fn detect_boot_mode() -> u64 {
    let mode = hostio::load_mode();
    let trimmed = trim_ascii(&mode);
    if trimmed == b"reset_then_crash_after_commit" {
        BOOT_MODE_RESET_AND_CRASH
    } else if trimmed == b"recover_after_crash" {
        BOOT_MODE_RECOVER
    } else {
        BOOT_MODE_VERIFY
    }
}

fn trim_ascii(bytes: &[u8]) -> &[u8] {
    let mut end = 0usize;
    while end < bytes.len() && bytes[end] != 0 && bytes[end] != b'\n' && bytes[end] != b'\r' {
        end += 1;
    }
    &bytes[..end]
}

fn checksum(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for &byte in bytes {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0000_0001_0000_01b3);
    }
    hash
}

const fn checksum_const(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let mut i = 0usize;
    while i < bytes.len() {
        hash ^= bytes[i] as u64;
        hash = hash.wrapping_mul(0x0000_0001_0000_01b3);
        i += 1;
    }
    hash
}

const fn padded_name(name: &[u8]) -> [u8; FILE_NAME_MAX] {
    let mut out = [0u8; FILE_NAME_MAX];
    let mut i = 0usize;
    while i < name.len() {
        out[i] = name[i];
        i += 1;
    }
    out
}

const fn padded_data(bytes: &[u8]) -> [u8; FILE_PAYLOAD_BYTES] {
    let mut out = [0u8; FILE_PAYLOAD_BYTES];
    let mut i = 0usize;
    while i < bytes.len() {
        out[i] = bytes[i];
        i += 1;
    }
    out
}

fn record_error(err: isize) {
    unsafe {
        DIAGNOSTICS.last_error = err as i64;
    }
    println!("[journal-error] code={}", err);
}
