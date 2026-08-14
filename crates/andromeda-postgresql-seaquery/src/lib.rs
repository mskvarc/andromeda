//! The generic PostgreSQL spelling of a SeaQuery statement.
//!
//! Everything here is dialect vocabulary with no NGSI-LD in it: the PostGIS,
//! `ltree`, `jsonb` and array operators sea-query has no native constructor for,
//! the casts and quoting a hand-written fragment needs, the typed bind values a
//! store binds, and the render-output struct a rendered statement leaves as.
//!
//! It knows nothing of the query languages ETSI GS CIM 009 v1.9.1 defines —
//! that translation is `andromeda-postgresql-query`, which is built on top of
//! these primitives — and it names no physical relation, which is
//! `andromeda-postgresql-schema`. `sqlx` stays out: a value leaves as a
//! [`bind::BindValue`] and the store binds it.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod bind;
pub mod built;
pub mod column_reference;
pub mod jsonb;
pub mod literal;
pub mod ltree;
pub mod operator;
pub mod postgis;
pub mod value_binding;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
    ],
    allow_workspace_dev_dependencies: [
    ],
    allow_production_dependencies: ["sea-query"],
}
