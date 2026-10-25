//! Typed request targeting for tenant-owned context branches.
//!
//! A tenant holds one live branch and zero or more Snapshot branches. This
//! crate names the branch a Core, Temporal, or subscription operation runs
//! against, independent of how a persistence backend realises the branch. The
//! PostgreSQL backend maps a Snapshot branch to a dedicated schema named after
//! the Snapshot's surrogate key; that mapping lives in the backend crate, not
//! here, so this crate carries no SQL vocabulary.
pub mod context_target;
pub mod snapshot_branch;
pub mod snapshot_key;
pub mod snapshot_target;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-ngsi-ld-lexicon",
    ],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: [],
}
