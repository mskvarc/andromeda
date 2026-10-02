//! Measuring the heap one step of a test takes.
//!
//! A test that pins how much memory a large payload costs on its way through
//! the broker installs the [`counting_allocator`] as its binary's global
//! allocator and reads the peak above a mark taken just before the step it
//! measures. Every such test measures the same way, so the allocator lives here
//! once.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod counting_allocator;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: [],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
