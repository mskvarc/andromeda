//! One tenant's attribute-name dictionary.
//!
//! Every relation that records an attribute — the temporal catalogue, the
//! history hypertable, the `current_attribute_values` projection, the
//! registration routing projection and the notification event queue — stores the
//! attribute's `INTEGER` dictionary key rather than its IRI. This crate owns
//! that dictionary: the table access, the per-tenant cache in front of it, the
//! bound on how far it may grow, and the port the stores hold it by.
//!
//! It is a crate of its own because the dictionary is shared substrate. The
//! current-state store, the temporal store and the atomic committer all resolve
//! names through the same tenant dictionary, and none of them is the right owner
//! of the other two's access to it.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod attribute_name_cache;
pub mod attribute_name_lookup;
pub mod dictionary_growth;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-ngsi-ld-lexicon",
        "andromeda-postgresql-query",
        "andromeda-postgresql-schema",
        "andromeda-postgresql-seaquery",
    ],
    allow_workspace_dev_dependencies: [
    ],
    allow_production_dependencies: ["sea-query", "sea-query-binder", "sqlx"],
}

#[cfg(test)]
mod port_contract_tests {
    use crate::{attribute_name_cache::AttributeNameCache, attribute_name_lookup::AttributeNameLookup};

    /// Compiles only for a type that implements the dictionary port.
    fn assert_attribute_name_lookup<T: AttributeNameLookup>() {}

    /// The cache is the dictionary port every store holds it by.
    #[test]
    fn the_cache_satisfies_the_dictionary_port() {
        assert_attribute_name_lookup::<AttributeNameCache>();
    }
}
