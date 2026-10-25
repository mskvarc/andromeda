//! The one owner of clause 4.5.23 Linked Entity retrieval.
//!
//! ETSI GS CIM 009 v1.9.1 states one Linked Entity retrieval and one clause
//! 4.21 projection that narrows it, and a per-operation rule deciding where a
//! request may reference a Linked Entity at all. Every Andromeda operation
//! that joins asks this crate, and only this crate, for each of those
//! answers: whether a reference is admitted, and how far a traversal walks and
//! what it excludes. A consumer supplies the operation it is and the
//! parameters it was given; it decides nothing about Linked Entities itself.
//!
//! The crate holds no store, HTTP or JSON-LD engine, so every consumer --
//! Core reads, notifications, and the temporal operations behind their
//! extension -- can depend on it.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod admission;
pub mod candidate;
pub mod edge;
pub mod graph;
pub mod join;
pub mod keyword;
pub mod narrowing;
pub mod plan;
pub mod representation;
pub mod source;
#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod test_graph;
pub mod traversal;
pub mod typed;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-model",
        "andromeda-ngsi-ld-query",
        "andromeda-ngsi-ld-representation",
    ],
    allow_workspace_dev_dependencies: [
    ],
    allow_production_dependencies: [],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
