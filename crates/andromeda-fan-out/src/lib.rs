//! Bounded fan-out, one future per item with at most a bounded number running
//! at once, collected as a stream.
//!
//! The stream's type names its futures by a declared `Send` bound rather than
//! by the closure that starts them. A request handler's future is proven
//! `Send` from that bound alone; through a closure the compiler would have to
//! prove the closure's futures `Send` for every lifetime they borrow, which it
//! cannot do on stable Rust.

/// Fan-outs in completion order and in item order.
pub mod bounded;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: [],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
