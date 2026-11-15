//! The PostgreSQL spelling of the NGSI-LD query languages.
//!
//! `andromeda-ngsi-ld-query` compiles a request into the languages ETSI GS CIM
//! 009 v1.9.1 defines; this crate says how each of them is written in
//! PostgreSQL. A `q` filter becomes a probe against the `current_attributes`
//! rows, a `georel` becomes a `PostGIS`
//! predicate, a `scopeQ` becomes an `lquery` match, a type selection becomes a
//! predicate over the interned `entity_type_name_keys` array or a walk of a
//! type-membership relation.
//!
//! The primitives those renderings are built from live below the NGSI-LD line:
//! the dialect vocabulary (PostGIS/ltree/jsonb operators, casts, bind, quoting)
//! is `andromeda-postgresql-seaquery`, the physical relation identifiers are
//! `andromeda-postgresql-schema`, and what a stored row means (a deletion, an
//! expiry, the side of an identity row, the default instance) is
//! `andromeda-postgresql-stored-form`. This crate holds only the translation of
//! the query languages onto them.
//!
//! Everything here is a fragment. Assembling fragments into a statement is the
//! job of the crate that owns the statement:
//! `andromeda-postgresql-context-query` for current entity state and context
//! source registrations, `andromeda-postgresql-temporal-query` for an entity's
//! temporal evolution. Both port groups render the same fragments, which is why
//! the fragments are here and not in either of them.

pub mod attribute_carriers;
pub mod attribute_instance_correlation;
pub mod dataset_predicate;
pub mod declared_operand;
pub mod entity_ordering;
pub mod entity_row_refs;
pub mod error;
pub mod geo_predicate;
pub mod history_source;
pub mod id_pattern_trigrams;
pub mod jsonb;
pub mod location_instances;
pub mod ltree;
pub mod one_time_gate;
pub mod operator;
pub mod ordering;
pub mod page_window;
pub mod projected_attribute_names;
pub mod projected_type_names;
pub mod q_predicate;
pub mod query_names;
pub mod scope_predicate;
pub mod scope_validity;
pub mod sort_key;
pub mod stored_geometry;
pub mod sub_attribute_operand_keys;
pub mod temporal_operand;
pub mod time_column;
pub mod time_interval_predicate;
pub mod type_membership;
pub mod type_predicate;
/// Which of the Scope events valid within a temporal window a probe reads.
pub mod window_reach;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-ngsi-ld-geometry",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-number",
        "andromeda-ngsi-ld-query",
        "andromeda-ngsi-ld-vocabulary",
        "andromeda-postgresql-encoding",
        "andromeda-postgresql-schema",
        "andromeda-postgresql-seaquery",
        "andromeda-postgresql-stored-form",
        "andromeda-postgresql-stored-values",
    ],
    allow_workspace_dev_dependencies: [
        "andromeda-postgresql-seaquery",
        "andromeda-q-corpus",
    ],
    allow_production_dependencies: ["sea-query"],
}
