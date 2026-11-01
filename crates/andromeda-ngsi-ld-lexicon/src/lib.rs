//! The lexical layer of NGSI-LD: the grammars every named value in the API must satisfy.
//!
//! ETSI GS CIM 009 v1.9.1 defines its resources on top of a small set of
//! lexical productions — the short-name grammar of clause 4.6.2, the scope
//! path of clause 4.18, the dotted attribute path of clause 4.5.3, the
//! `DateTime`, `Date` and `Time` forms of clause 4.6.3, and the URI that names
//! every resource.
//! This crate owns those productions and the newtypes built directly on them,
//! so that a value which has passed one of these parsers is known-valid
//! everywhere above.
//!
//! It sits below the data model rather than inside it: the query languages, the
//! HTTP binding and the stores all name these types without depending on the
//! resources of clause 5.2.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

#[macro_use]
mod validated_name;

pub mod aggr_method;
pub mod attribute_name;
pub mod attribute_null_encoding;
pub mod attribute_type;
pub mod coerced_term;
pub mod constant_string;
pub mod context_source_pseudonym;
pub mod datatype;
pub mod dereferenceable_uri;
pub mod duration;
pub mod entity_type_name;
pub mod identifier;
pub mod identifier_length;
pub mod instance_identity;
pub mod name;
pub mod ngsi_ld_urn;
pub mod non_empty_text;
pub mod positive_milliseconds;
pub mod reference;
pub mod scope;
pub mod spec_version;
pub mod temporal_format;
pub mod tenant;
pub mod tenant_database_name;
pub mod time;
pub mod time_prop;
pub mod time_rel;
pub mod unit_code_admission;
pub mod unit_code_resolution;
pub mod uri;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-configuration",
        "andromeda-ngsi-ld-number",
        "andromeda-ngsi-ld-vocabulary",
    ],
    allow_workspace_dev_dependencies: [
    ],
    allow_production_dependencies: [],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
