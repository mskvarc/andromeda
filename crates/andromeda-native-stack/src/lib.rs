//! Keeping a recursion over client input off the end of the native stack.
//!
//! A few readings recurse once per level of what a client wrote: the reading of a compact attribute recurses into each sub-attribute, and the assembly of its expansion does the same. Each level stays within a bound the input's syntax sets, but those readings run on a Tokio worker inside a request handler that has already spent part of the worker's stack, and an unoptimized level takes tens of kibibytes, so the depth alone cannot decide whether the stack suffices.
//!
//! A recursion point runs its callee through [`on_sufficient_stack`], which moves it to a freshly allocated segment when little of the current stack is left. What bounds a recursion stays what bounds it; this only lets the bound be reached on any thread, as `jsonld_syntax::native_stack` does for the JSON-LD algorithms.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

/// Stack left below which a recursion level moves to a fresh segment: well above what one level of the readings takes in an unoptimized build.
const RED_ZONE: usize = 128 * 1024;

/// Size of each fresh stack segment.
const SEGMENT: usize = 2 * 1024 * 1024;

/// Runs `callee`, on a fresh stack segment when the current one is nearly
/// used.
pub fn on_sufficient_stack<R>(callee: impl FnOnce() -> R) -> R {
    stacker::maybe_grow(RED_ZONE, SEGMENT, callee)
}

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: [],
}

#[cfg(test)]
mod tests {
    use crate::on_sufficient_stack;
    use std::{hint::black_box, thread};

    /// Each level of this recursion holds a frame of 16 KiB, so 1,024 levels need 16 MiB of stack, far more than the 256 KiB thread it runs on.
    fn depth_of(levels: usize) -> usize {
        let frame = [0_u8; 16 * 1024];
        let below = if levels == 0 { 0 } else { on_sufficient_stack(|| depth_of(levels - 1)) };
        below + usize::from(black_box(frame)[0] == 0)
    }

    #[test]
    fn a_recursion_deeper_than_the_thread_stack_completes() {
        let levels = thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(|| depth_of(1_024))
            .expect("the thread starts")
            .join()
            .expect("the recursion completes");
        pretty_assertions::assert_eq!(levels, 1_025);
    }
}
