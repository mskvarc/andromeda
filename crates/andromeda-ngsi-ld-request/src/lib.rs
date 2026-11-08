//! What an NGSI-LD client asked for, as the specification spells it.
//!
//! ETSI GS CIM 009 v1.9.1 defines a request two ways: as query parameters on a
//! `GET` (clause 6.3), and as a JSON body on the `POST` equivalents (clauses
//! 5.7.2, 6.23, 6.34.3.2). They are the same request, and the body form is
//! defined by conversion into the parameter form, so both live here together
//! with the conversion between them.
//!
//! Nothing here is a web framework's type. A parameter set is deserialised from
//! a query string with `serde_urlencoded` and a body with `serde`; extracting
//! either from a live request is the binding layer's job. That is what lets the
//! forwarding client build a request for another broker out of the same types
//! the server reads one with.
//!
//! `deny_unknown_fields` is on every parameter struct deliberately: Andromeda
//! validates requests strictly and rejects one it does not fully understand, so
//! a silently ignored `?limitt=1000` is a wrong answer rather than a slow one.

pub mod attribute_deletion;
pub mod body;
pub mod deferred_split_filters;
pub mod discovery;
pub mod empty;
pub mod entity;
pub mod entity_map_lifetime;
pub mod entity_operations;
pub mod entity_write;
pub mod error;
pub mod fragment_entity_id;
pub mod geometry_media_type;
pub mod jsonld_context;
pub mod language_filter;
pub mod language_tag;
pub mod local;
pub mod options;
pub mod options_token;
pub mod pagination;
pub mod query;
pub mod query_narrowing;
pub mod query_parameter;
pub mod registration_retrieval;
pub mod representation_format;
pub mod representation_parameter;
pub mod subscription_listing;
pub mod sys_attrs_parameters;
pub mod temporal_instance_fragment;
pub mod unsupported_representation;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-configuration",
        "andromeda-http-headers",
        "andromeda-linked-retrieval",
        "andromeda-ngsi-ld-geometry",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-model",
        "andromeda-ngsi-ld-query",
        "andromeda-ngsi-ld-representation",
    ],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: [],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
