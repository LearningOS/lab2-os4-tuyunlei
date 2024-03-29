//! Memory management implementation
//!
//! SV39 page-based virtual-memory architecture for RV64 systems, and
//! everything about memory management, like frame allocator, page table,
//! map area and memory set, is implemented here.
//!
//! Every task or process has a memory_set to control its virtual memory.


pub use address::{PhysAddr, PhysPageNum, VirtAddr, VirtPageNum};
pub use address::{StepByOne, VPNRange};
pub use frame_allocator::{frame_alloc, FrameTracker};
pub use memory_set::{KERNEL_SPACE, MapPermission, MemorySet};
pub use memory_set::remap_test;
pub use page_table::{PageTableEntry, translated_byte_buffer, copy_data_into_space};
use page_table::{PageTable, PTEFlags};

mod address;
mod frame_allocator;
mod heap_allocator;
mod memory_set;
mod page_table;

/// initiate heap allocator, frame allocator and kernel space
pub fn init() {
    heap_allocator::init_heap();
    frame_allocator::init_frame_allocator();
    KERNEL_SPACE.lock().activate();
}
