//! How an NGSI-LD broker reports a failure.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.5.2 defines the twelve error types, each with
//! a canonical type IRI; clause 6.3.2 assigns each an HTTP status and clause
//! 5.5.3 its title. Clause 5.2.16 gives batch operations a per-entity result
//! shape built from the same
//! categories. Both are wire contracts: a client reads them, a Context Source
//! writes them, and a broker forwarding an operation has to parse one back.
//!
//! What is *not* here is Andromeda's own error type, or any conversion from it.
//! A category is what the specification says a failure is called; deciding
//! which category a particular internal failure falls into is broker policy and
//! belongs to the layer that holds that failure. Nothing here names a
//! framework, so the same shapes serve the server answering a request and the
//! forwarding client reading someone else's answer.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod batch_operation;
pub mod error_type;
pub mod problem_detail;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-ngsi-ld-lexicon",
    ],
    allow_workspace_dev_dependencies: [
    ],
    allow_production_dependencies: [],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
