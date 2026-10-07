//! One tenant's name dictionaries.
//!
//! Every relation that records an attribute (the temporal catalogue, the
//! history hypertable, the `current_attribute_values` projection, the
//! registration routing projection and the notification event queue) stores
//! the attribute's `INTEGER` dictionary key rather than its IRI, and every
//! relation that records an entity's types (`entities`, `temporal_entities`)
//! stores an `INTEGER[]` of type keys. This crate owns those dictionaries: the
//! table access, the bounded per-tenant cache in front of each, that cache's
//! configuration, and the port the stores hold them by.
//!
//! It is a crate of its own because the dictionaries are shared substrate. The
//! current-state store, the temporal store and the atomic committer all
//! resolve names through the same tenant dictionaries, and none of them is the
//! right owner of the other two's access to them.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod attribute_names;
mod bounded_keys;
pub mod dictionary_relation;
mod dictionary_rows;
mod dictionary_statements;
pub mod entity_type_names;
pub mod name_cache;
pub mod name_cache_config;
pub mod name_lookup;
pub mod schema_epoch;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-configuration-scalars",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-persistence-failure",
        "andromeda-postgresql-pool",
        "andromeda-postgresql-query",
        "andromeda-postgresql-schema",
        "andromeda-postgresql-seaquery",
    ],
    allow_workspace_dev_dependencies: [
    ],
    allow_production_dependencies: ["sea-query", "sea-query-sqlx", "sqlx"],
}

#[cfg(test)]
mod port_contract_tests {
    use crate::{name_cache::NameCache, name_lookup::NameLookup};
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
    use andromeda_postgresql_query::dictionary::interned_name::InternedName;

    /// Compiles only for a type that implements the dictionary port for `N`.
    fn assert_name_lookup<N: InternedName, T: NameLookup<N>>() {}

    /// The cache is the dictionary port every store holds it by, for both
    /// interned name types.
    #[test]
    fn the_cache_satisfies_the_dictionary_port_for_both_name_types() {
        assert_name_lookup::<AttributeName, NameCache<AttributeName>>();
        assert_name_lookup::<EntityTypeName, NameCache<EntityTypeName>>();
    }
}
