//! The system allocator, counting the bytes it hands out.
//!
//! A test binary installs it as its global allocator,
//!
//! ```ignore
//! #[global_allocator]
//! static ALLOCATOR: CountingAllocator = CountingAllocator::new();
//! ```
//!
//! and measures one step between [`CountingAllocator::mark`] and
//! [`CountingAllocator::peak_above`] or [`CountingAllocator::allocations_since`].
//! The allocator counts every allocation of
//! the binary, so a measured step must not run beside another test of the same
//! binary.

use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicUsize, Ordering},
};

/// The system allocator, counting the bytes it holds, the most it has held at
/// once since the last [`CountingAllocator::mark`], and how many allocations it
/// has made.
#[derive(Debug, Default)]
pub struct CountingAllocator {
    /// Bytes currently allocated.
    current: AtomicUsize,
    /// The most bytes allocated at once since the last mark.
    peak: AtomicUsize,
    /// Allocations made, counting each reallocation as one.
    allocations: AtomicUsize,
}

/// What was allocated when a measurement began.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeapMark {
    /// Bytes held.
    bytes: usize,
    /// Allocations made.
    allocations: usize,
}

impl CountingAllocator {
    /// An allocator that has counted nothing yet.
    #[must_use]
    pub const fn new() -> CountingAllocator {
        CountingAllocator {
            current: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
            allocations: AtomicUsize::new(0),
        }
    }

    /// Begin a measurement: the peak restarts at what is allocated now.
    #[must_use]
    pub fn mark(&self) -> HeapMark {
        let current = self.current.load(Ordering::SeqCst);
        self.peak.store(current, Ordering::SeqCst);
        HeapMark {
            bytes: current,
            allocations: self.allocations.load(Ordering::SeqCst),
        }
    }

    /// The most bytes held at once since `mark`, above what was held at it.
    #[must_use]
    pub fn peak_above(&self, mark: HeapMark) -> usize {
        self.peak.load(Ordering::SeqCst).saturating_sub(mark.bytes)
    }

    /// How many allocations were made since `mark`.
    #[must_use]
    pub fn allocations_since(&self, mark: HeapMark) -> usize {
        self.allocations.load(Ordering::SeqCst).saturating_sub(mark.allocations)
    }

    /// Count `size` more bytes and raise the peak.
    fn grow(&self, size: usize) {
        self.allocations.fetch_add(1, Ordering::SeqCst);
        let current = self.current.fetch_add(size, Ordering::SeqCst).saturating_add(size);
        self.peak.fetch_max(current, Ordering::SeqCst);
    }

    /// Count `size` fewer bytes.
    fn shrink(&self, size: usize) {
        self.current.fetch_sub(size, Ordering::SeqCst);
    }
}

// SAFETY: every call forwards to the system allocator with the caller's own
// arguments; the counters only observe sizes.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: forwarded unchanged.
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            self.grow(layout.size());
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: forwarded unchanged.
        unsafe { System.dealloc(pointer, layout) };
        self.shrink(layout.size());
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: forwarded unchanged.
        let moved = unsafe { System.realloc(pointer, layout, new_size) };
        if !moved.is_null() {
            self.grow(new_size);
            self.shrink(layout.size());
        }
        moved
    }
}

#[cfg(test)]
mod tests {
    use crate::counting_allocator::CountingAllocator;
    use std::alloc::{GlobalAlloc, Layout};

    #[test]
    fn the_peak_above_a_mark_is_the_most_held_at_once_since_it() {
        let allocator = CountingAllocator::new();
        let layout = Layout::from_size_align(4096, 8).expect("a valid layout");
        let mark = allocator.mark();
        // SAFETY: the layout is non-zero; each block is freed with the layout
        // it was allocated with.
        unsafe {
            let first = allocator.alloc(layout);
            let second = allocator.alloc(layout);
            allocator.dealloc(first, layout);
            allocator.dealloc(second, layout);
        }
        pretty_assertions::assert_eq!(allocator.peak_above(mark), 8192);
        pretty_assertions::assert_eq!(allocator.allocations_since(mark), 2);
        let later = allocator.mark();
        pretty_assertions::assert_eq!(allocator.peak_above(later), 0, "a new mark restarts the peak");
    }
}
