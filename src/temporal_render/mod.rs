//! Rendering the temporal evolution of an entity (clauses 4.5.5, 5.7.3, 6.3.10).
//!
//! The other renderings live in `andromeda-ngsi-ld-render`; these do not,
//! because they still read `sqlx::postgres::PgRow` directly. That is the store's
//! row format reaching into the presentation layer, and it is what stops this
//! half from being a specification crate. It ends when `service::temporal`
//! stops composing SQL and the temporal store returns a neutral row instead;
//! these five renderers then join the temporal API they belong to.
//!
//! Each renderer owns one clause, and the two modules below them own the two
//! decodings they share.

pub mod aggregation;
pub mod array_normalizer;
pub mod entity;
pub mod geojson_ld;
pub mod history_row;
pub mod simplify;
pub mod values;
