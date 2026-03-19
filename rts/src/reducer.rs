use std::{mem::MaybeUninit, ptr};

#[repr(C)]
pub struct Value {
    pub info: *const InfoTable,
    pub payload: *mut u8,
}

#[repr(C)]
pub struct InfoTable {
    pub entry: usize,
    pub tag: u32,
    pub layout: LayoutInfo,
}

#[repr(C)]
pub struct LayoutInfo {
    pub size: usize,
    pub pointers: usize,
    pub pointer_offsets: [usize],
}

#[repr(C)]
pub struct Heap {
    from_space: *mut u8,
    to_space: *mut u8,
    alloc_ptr: usize,
    scan_ptr: usize,
    size: usize,
}

impl Heap {
    pub fn new(size: usize) -> Self {
        Heap {
            from_space: unsafe {
                let layout = std::alloc::Layout::from_size_align(size, 8).unwrap();
                std::alloc::alloc(layout)
            },
            to_space: unsafe {
                let layout = std::alloc::Layout::from_size_align(size, 8).unwrap();
                std::alloc::alloc(layout)
            },
            alloc_ptr: 0,
            scan_ptr: 0,
            size,
        }
    }
}
