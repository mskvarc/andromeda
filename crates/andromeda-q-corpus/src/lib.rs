//! A fixed corpus of `q` filters over a fixed set of Entities, each filter
//! with the Entities ETSI GS CIM 009 v1.9.1 clause 4.9 has it select.
//!
//! A broker answers a `q` in its store for a query and over the Entity a write
//! produced for a Subscription. The corpus states once what each filter
//! means, and every test that holds an execution to the clause reads it: the
//! statements the store is sent for each filter, and the black-box suite,
//! which asks Query Entities and a Subscription the same filter and expects
//! both to answer the stated Entities. It is data only, so the suite, which
//! reaches the broker over the network alone, can read it beside the broker's
//! own tests.
//!
//! The attribute names are short terms of the requester's `@context`.

pub mod corpus;
pub mod corpus_entity;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: [],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
