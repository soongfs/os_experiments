use crate::abi::{
    FsStat, EEXIST, EFBIG, EINVAL, EISDIR, ENAMETOOLONG, ENOENT, ENOSPC, ENOTDIR, ENOTEMPTY,
    FS_BLOCK_SIZE, FS_DEVICE_ID, FS_DIRECT_POINTERS, FS_KIND_DIR, FS_KIND_FILE, FS_LEVEL_DIRECT,
    FS_LEVEL_DOUBLE, FS_LEVEL_SINGLE, FS_LEVEL_TRIPLE, FS_MAX_DIR_ENTRIES, FS_NAME_MAX,
    FS_PATH_MAX, FS_POINTERS_PER_BLOCK,
};
use crate::{FS_MAX_DATA_BLOCKS, FS_MAX_INODES, FS_MAX_PTR_BLOCKS};

const ROOT_INODE: usize = 0;
const NULL_SLOT: u32 = 0;
const MAX_FILE_BYTES: u64 =
    ((FS_DIRECT_POINTERS + FS_POINTERS_PER_BLOCK + FS_POINTERS_PER_BLOCK * FS_POINTERS_PER_BLOCK
        + FS_POINTERS_PER_BLOCK * FS_POINTERS_PER_BLOCK * FS_POINTERS_PER_BLOCK)
        * FS_BLOCK_SIZE) as u64;

#[derive(Clone, Copy)]
struct Inode {
    used: bool,
    kind: u8,
    highest_level: u8,
    name_len: u8,
    parent: u16,
    _padding: u16,
    name: [u8; FS_NAME_MAX],
    inode_number: u64,
    size_bytes: u64,
    blocks_used: u64,
    child_count: u16,
    _child_padding: u16,
    created_us: u64,
    modified_us: u64,
    children: [u16; FS_MAX_DIR_ENTRIES],
    direct: [u32; FS_DIRECT_POINTERS],
    single: u32,
    double: u32,
    triple: u32,
}

impl Inode {
    const fn empty() -> Self {
        Self {
            used: false,
            kind: 0,
            highest_level: FS_LEVEL_DIRECT,
            name_len: 0,
            parent: 0,
            _padding: 0,
            name: [0; FS_NAME_MAX],
            inode_number: 0,
            size_bytes: 0,
            blocks_used: 0,
            child_count: 0,
            _child_padding: 0,
            created_us: 0,
            modified_us: 0,
            children: [0; FS_MAX_DIR_ENTRIES],
            direct: [0; FS_DIRECT_POINTERS],
            single: 0,
            double: 0,
            triple: 0,
        }
    }
}

static mut INODES: [Inode; FS_MAX_INODES] = [Inode::empty(); FS_MAX_INODES];
static mut DATA_BLOCKS: [[u8; FS_BLOCK_SIZE]; FS_MAX_DATA_BLOCKS] =
    [[0; FS_BLOCK_SIZE]; FS_MAX_DATA_BLOCKS];
static mut PTR_BLOCKS: [[u32; FS_POINTERS_PER_BLOCK]; FS_MAX_PTR_BLOCKS] =
    [[0; FS_POINTERS_PER_BLOCK]; FS_MAX_PTR_BLOCKS];
static mut NEXT_INODE: usize = 1;
static mut NEXT_DATA_BLOCK: usize = 0;
static mut NEXT_PTR_BLOCK: usize = 0;
static mut METADATA_CLOCK_US: u64 = 1;

pub fn init() {
    unsafe {
        NEXT_INODE = 1;
        NEXT_DATA_BLOCK = 0;
        NEXT_PTR_BLOCK = 0;
        METADATA_CLOCK_US = 1;
        INODES[ROOT_INODE] = Inode::empty();
        INODES[ROOT_INODE].used = true;
        INODES[ROOT_INODE].kind = FS_KIND_DIR;
        INODES[ROOT_INODE].highest_level = FS_LEVEL_DIRECT;
        INODES[ROOT_INODE].name_len = 1;
        INODES[ROOT_INODE].parent = ROOT_INODE as u16;
        INODES[ROOT_INODE].name[0] = b'/';
        INODES[ROOT_INODE].inode_number = 1;
        INODES[ROOT_INODE].created_us = next_metadata_time();
        INODES[ROOT_INODE].modified_us = INODES[ROOT_INODE].created_us;
    }
}

pub fn create_dir(path: &[u8]) -> isize {
    create_node(path, FS_KIND_DIR)
}

pub fn create_file(path: &[u8]) -> isize {
    create_node(path, FS_KIND_FILE)
}

pub fn write_at(path: &[u8], offset: usize, src: &[u8]) -> isize {
    let inode_index = match lookup_path(path) {
        Ok(index) => index,
        Err(err) => return err,
    };

    unsafe {
        if INODES[inode_index].kind != FS_KIND_FILE {
            return EISDIR;
        }
    }

    let end_offset = match offset.checked_add(src.len()) {
        Some(value) => value,
        None => return EFBIG,
    };
    if end_offset as u64 > MAX_FILE_BYTES {
        return EFBIG;
    }

    let mut copied = 0usize;
    while copied < src.len() {
        let file_offset = offset + copied;
        let logical_block = file_offset / FS_BLOCK_SIZE;
        let block_offset = file_offset % FS_BLOCK_SIZE;
        let copy_len = core::cmp::min(FS_BLOCK_SIZE - block_offset, src.len() - copied);
        let (data_block_index, level, newly_allocated) =
            match resolve_data_block(inode_index, logical_block, true) {
                Ok(result) => result,
                Err(err) => return err,
            };

        unsafe {
            DATA_BLOCKS[data_block_index][block_offset..block_offset + copy_len]
                .copy_from_slice(&src[copied..copied + copy_len]);
            if newly_allocated {
                INODES[inode_index].blocks_used += 1;
            }
            if level > INODES[inode_index].highest_level {
                INODES[inode_index].highest_level = level;
            }
        }

        copied += copy_len;
    }

    unsafe {
        if end_offset as u64 > INODES[inode_index].size_bytes {
            INODES[inode_index].size_bytes = end_offset as u64;
        }
        INODES[inode_index].modified_us = next_metadata_time();
    }

    copied as isize
}

pub fn read_at(path: &[u8], offset: usize, dst: &mut [u8]) -> isize {
    let inode_index = match lookup_path(path) {
        Ok(index) => index,
        Err(err) => return err,
    };

    let file_size = unsafe {
        if INODES[inode_index].kind != FS_KIND_FILE {
            return EISDIR;
        }
        INODES[inode_index].size_bytes as usize
    };

    if offset >= file_size {
        return 0;
    }

    let mut copied = 0usize;
    let available = core::cmp::min(dst.len(), file_size - offset);
    while copied < available {
        let file_offset = offset + copied;
        let logical_block = file_offset / FS_BLOCK_SIZE;
        let block_offset = file_offset % FS_BLOCK_SIZE;
        let copy_len = core::cmp::min(FS_BLOCK_SIZE - block_offset, available - copied);

        match resolve_data_block(inode_index, logical_block, false) {
            Ok((data_block_index, _, _)) => unsafe {
                dst[copied..copied + copy_len].copy_from_slice(
                    &DATA_BLOCKS[data_block_index][block_offset..block_offset + copy_len],
                );
            },
            Err(ENOENT) => {
                for byte in &mut dst[copied..copied + copy_len] {
                    *byte = 0;
                }
            }
            Err(err) => return err,
        }

        copied += copy_len;
    }

    copied as isize
}

pub fn stat(path: &[u8], stat: &mut FsStat) -> isize {
    let inode_index = match lookup_path(path) {
        Ok(index) => index,
        Err(err) => return err,
    };

    unsafe {
        stat.kind = INODES[inode_index].kind;
        stat.highest_level = INODES[inode_index].highest_level;
        stat.inode_number = INODES[inode_index].inode_number;
        stat.device_id = FS_DEVICE_ID;
        stat.size_bytes = INODES[inode_index].size_bytes;
        stat.blocks_used = INODES[inode_index].blocks_used;
        stat.child_count = INODES[inode_index].child_count as u64;
        stat.created_us = INODES[inode_index].created_us;
        stat.modified_us = INODES[inode_index].modified_us;
    }

    0
}

pub fn remove(path: &[u8]) -> isize {
    let (parent_path, name) = match split_parent_and_name(path) {
        Ok(result) => result,
        Err(err) => return err,
    };

    let parent_index = match lookup_path(parent_path) {
        Ok(index) => index,
        Err(err) => return err,
    };
    let child_index = match find_child(parent_index, name) {
        Some(index) => index,
        None => return ENOENT,
    };

    unsafe {
        if INODES[child_index].kind == FS_KIND_DIR && INODES[child_index].child_count != 0 {
            return ENOTEMPTY;
        }
    }

    remove_child(parent_index, child_index);
    unsafe {
        INODES[child_index] = Inode::empty();
        INODES[parent_index].modified_us = next_metadata_time();
    }
    0
}

#[allow(dead_code)]
pub fn list_dir(path: &[u8], dst: &mut [u8]) -> isize {
    let inode_index = match lookup_path(path) {
        Ok(index) => index,
        Err(err) => return err,
    };

    unsafe {
        if INODES[inode_index].kind != FS_KIND_DIR {
            return ENOTDIR;
        }
    }

    let mut written = 0usize;
    unsafe {
        let child_count = INODES[inode_index].child_count as usize;
        for child_slot in 0..child_count {
            let child_index = INODES[inode_index].children[child_slot] as usize;
            let name_len = INODES[child_index].name_len as usize;
            let required = name_len + 1;
            if written + required > dst.len() {
                return ENOSPC;
            }
            dst[written..written + name_len]
                .copy_from_slice(&INODES[child_index].name[..name_len]);
            written += name_len;
            dst[written] = b'\n';
            written += 1;
        }
    }

    written as isize
}

fn create_node(path: &[u8], kind: u8) -> isize {
    let (parent_path, name) = match split_parent_and_name(path) {
        Ok(result) => result,
        Err(err) => return err,
    };
    let parent_index = match lookup_path(parent_path) {
        Ok(index) => index,
        Err(err) => return err,
    };

    unsafe {
        if INODES[parent_index].kind != FS_KIND_DIR {
            return ENOTDIR;
        }
    }
    if find_child(parent_index, name).is_some() {
        return EEXIST;
    }

    let new_inode = match allocate_inode(kind, parent_index, name) {
        Ok(index) => index,
        Err(err) => return err,
    };
    add_child(parent_index, new_inode)
}

fn lookup_path(path: &[u8]) -> Result<usize, isize> {
    validate_path(path)?;

    let end = trimmed_path_end(path);
    if end == 1 {
        return Ok(ROOT_INODE);
    }

    let mut current = ROOT_INODE;
    let mut index = 1usize;
    while index < end {
        while index < end && path[index] == b'/' {
            index += 1;
        }
        if index >= end {
            break;
        }
        let component_start = index;
        while index < end && path[index] != b'/' {
            index += 1;
        }
        let component = &path[component_start..index];
        current = match find_child(current, component) {
            Some(child) => child,
            None => return Err(ENOENT),
        };
        unsafe {
            if index < end && INODES[current].kind != FS_KIND_DIR {
                return Err(ENOTDIR);
            }
        }
    }

    Ok(current)
}

fn validate_path(path: &[u8]) -> Result<(), isize> {
    if path.is_empty() {
        return Err(EINVAL);
    }
    if path.len() >= FS_PATH_MAX {
        return Err(ENAMETOOLONG);
    }
    if path[0] != b'/' {
        return Err(EINVAL);
    }

    let end = trimmed_path_end(path);
    let mut index = 1usize;
    while index < end {
        while index < end && path[index] == b'/' {
            index += 1;
        }
        if index >= end {
            break;
        }
        let component_start = index;
        while index < end && path[index] != b'/' {
            index += 1;
        }
        let component = &path[component_start..index];
        if component.is_empty() {
            return Err(EINVAL);
        }
        if component.len() >= FS_NAME_MAX {
            return Err(ENAMETOOLONG);
        }
        if component == b"." || component == b".." {
            return Err(EINVAL);
        }
    }

    Ok(())
}

fn trimmed_path_end(path: &[u8]) -> usize {
    let mut end = path.len();
    while end > 1 && path[end - 1] == b'/' {
        end -= 1;
    }
    end
}

fn split_parent_and_name(path: &[u8]) -> Result<(&[u8], &[u8]), isize> {
    validate_path(path)?;

    let end = trimmed_path_end(path);
    if end == 1 {
        return Err(EINVAL);
    }

    let mut split = end - 1;
    while split > 0 && path[split] != b'/' {
        split -= 1;
    }

    let name = &path[split + 1..end];
    if name.is_empty() {
        return Err(EINVAL);
    }

    let parent = if split == 0 { &path[..1] } else { &path[..split] };
    Ok((parent, name))
}

fn find_child(parent_index: usize, name: &[u8]) -> Option<usize> {
    unsafe {
        let child_count = INODES[parent_index].child_count as usize;
        for slot in 0..child_count {
            let child_index = INODES[parent_index].children[slot] as usize;
            let child_name_len = INODES[child_index].name_len as usize;
            if child_name_len == name.len() && INODES[child_index].name[..child_name_len] == *name {
                return Some(child_index);
            }
        }
    }

    None
}

fn allocate_inode(kind: u8, parent_index: usize, name: &[u8]) -> Result<usize, isize> {
    unsafe {
        if NEXT_INODE >= FS_MAX_INODES {
            return Err(ENOSPC);
        }

        let inode_index = NEXT_INODE;
        let timestamp = next_metadata_time();
        NEXT_INODE += 1;
        INODES[inode_index] = Inode::empty();
        INODES[inode_index].used = true;
        INODES[inode_index].kind = kind;
        INODES[inode_index].name_len = name.len() as u8;
        INODES[inode_index].parent = parent_index as u16;
        INODES[inode_index].name[..name.len()].copy_from_slice(name);
        INODES[inode_index].inode_number = (inode_index + 1) as u64;
        INODES[inode_index].created_us = timestamp;
        INODES[inode_index].modified_us = timestamp;
        Ok(inode_index)
    }
}

fn add_child(parent_index: usize, child_index: usize) -> isize {
    unsafe {
        let child_count = INODES[parent_index].child_count as usize;
        if child_count >= FS_MAX_DIR_ENTRIES {
            return ENOSPC;
        }
        INODES[parent_index].children[child_count] = child_index as u16;
        INODES[parent_index].child_count += 1;
        INODES[parent_index].modified_us = next_metadata_time();
    }
    0
}

fn remove_child(parent_index: usize, child_index: usize) {
    unsafe {
        let child_count = INODES[parent_index].child_count as usize;
        for slot in 0..child_count {
            if INODES[parent_index].children[slot] as usize == child_index {
                for shift in slot..child_count - 1 {
                    INODES[parent_index].children[shift] = INODES[parent_index].children[shift + 1];
                }
                INODES[parent_index].children[child_count - 1] = 0;
                INODES[parent_index].child_count -= 1;
                return;
            }
        }
    }
}

fn resolve_data_block(
    inode_index: usize,
    logical_block: usize,
    allocate: bool,
) -> Result<(usize, u8, bool), isize> {
    if logical_block < FS_DIRECT_POINTERS {
        return resolve_direct_slot(inode_index, logical_block, allocate);
    }

    let single_limit = FS_DIRECT_POINTERS + FS_POINTERS_PER_BLOCK;
    if logical_block < single_limit {
        return resolve_single_slot(inode_index, logical_block - FS_DIRECT_POINTERS, allocate);
    }

    let double_start = single_limit;
    let double_capacity = FS_POINTERS_PER_BLOCK * FS_POINTERS_PER_BLOCK;
    if logical_block < double_start + double_capacity {
        return resolve_double_slot(inode_index, logical_block - double_start, allocate);
    }

    let triple_start = double_start + double_capacity;
    let triple_capacity = FS_POINTERS_PER_BLOCK * FS_POINTERS_PER_BLOCK * FS_POINTERS_PER_BLOCK;
    if logical_block < triple_start + triple_capacity {
        return resolve_triple_slot(inode_index, logical_block - triple_start, allocate);
    }

    Err(EFBIG)
}

fn resolve_direct_slot(
    inode_index: usize,
    slot_index: usize,
    allocate: bool,
) -> Result<(usize, u8, bool), isize> {
    unsafe { resolve_data_slot(&mut INODES[inode_index].direct[slot_index], allocate, FS_LEVEL_DIRECT) }
}

fn resolve_single_slot(
    inode_index: usize,
    slot_index: usize,
    allocate: bool,
) -> Result<(usize, u8, bool), isize> {
    let ptr_block_index = unsafe { ensure_ptr_block(&mut INODES[inode_index].single, allocate)? };
    unsafe { resolve_data_slot(&mut PTR_BLOCKS[ptr_block_index][slot_index], allocate, FS_LEVEL_SINGLE) }
}

fn resolve_double_slot(
    inode_index: usize,
    logical_index: usize,
    allocate: bool,
) -> Result<(usize, u8, bool), isize> {
    let outer = logical_index / FS_POINTERS_PER_BLOCK;
    let inner = logical_index % FS_POINTERS_PER_BLOCK;
    let root_ptr = unsafe { ensure_ptr_block(&mut INODES[inode_index].double, allocate)? };
    let leaf_ptr = unsafe {
        let slot = &mut PTR_BLOCKS[root_ptr][outer];
        ensure_ptr_block(slot, allocate)?
    };
    unsafe { resolve_data_slot(&mut PTR_BLOCKS[leaf_ptr][inner], allocate, FS_LEVEL_DOUBLE) }
}

fn resolve_triple_slot(
    inode_index: usize,
    logical_index: usize,
    allocate: bool,
) -> Result<(usize, u8, bool), isize> {
    let outer = logical_index / (FS_POINTERS_PER_BLOCK * FS_POINTERS_PER_BLOCK);
    let remainder = logical_index % (FS_POINTERS_PER_BLOCK * FS_POINTERS_PER_BLOCK);
    let middle = remainder / FS_POINTERS_PER_BLOCK;
    let inner = remainder % FS_POINTERS_PER_BLOCK;

    let root_ptr = unsafe { ensure_ptr_block(&mut INODES[inode_index].triple, allocate)? };
    let middle_ptr = unsafe {
        let slot = &mut PTR_BLOCKS[root_ptr][outer];
        ensure_ptr_block(slot, allocate)?
    };
    let leaf_ptr = unsafe {
        let slot = &mut PTR_BLOCKS[middle_ptr][middle];
        ensure_ptr_block(slot, allocate)?
    };
    unsafe { resolve_data_slot(&mut PTR_BLOCKS[leaf_ptr][inner], allocate, FS_LEVEL_TRIPLE) }
}

unsafe fn ensure_ptr_block(slot: &mut u32, allocate: bool) -> Result<usize, isize> {
    if *slot == NULL_SLOT {
        if !allocate {
            return Err(ENOENT);
        }
        if NEXT_PTR_BLOCK >= FS_MAX_PTR_BLOCKS {
            return Err(ENOSPC);
        }
        let ptr_index = NEXT_PTR_BLOCK;
        NEXT_PTR_BLOCK += 1;
        PTR_BLOCKS[ptr_index].fill(NULL_SLOT);
        *slot = stored_index(ptr_index);
    }

    Ok(real_index(*slot))
}

unsafe fn resolve_data_slot(
    slot: &mut u32,
    allocate: bool,
    level: u8,
) -> Result<(usize, u8, bool), isize> {
    if *slot == NULL_SLOT {
        if !allocate {
            return Err(ENOENT);
        }
        if NEXT_DATA_BLOCK >= FS_MAX_DATA_BLOCKS {
            return Err(ENOSPC);
        }
        let data_index = NEXT_DATA_BLOCK;
        NEXT_DATA_BLOCK += 1;
        DATA_BLOCKS[data_index].fill(0);
        *slot = stored_index(data_index);
        return Ok((data_index, level, true));
    }

    Ok((real_index(*slot), level, false))
}

fn stored_index(index: usize) -> u32 {
    (index as u32) + 1
}

fn real_index(stored: u32) -> usize {
    (stored - 1) as usize
}

unsafe fn next_metadata_time() -> u64 {
    let value = METADATA_CLOCK_US;
    METADATA_CLOCK_US += 1;
    value
}
