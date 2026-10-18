//! The table aliases and projection output name the temporal statement reads
//! its relations under.
//!
//! These are query-construction identifiers, not physical schema: they name how
//! stage 1 aliases the relations it selects from, and change when the statement
//! restructures its joins rather than when the database schema does. The
//! physical tables they alias live in `andromeda-postgresql-schema`.

use sea_query::Iden;

/// Alias `s` for the stream source, or the `temporal_attribute_streams` table.
#[derive(Iden, Clone, Copy)]
#[iden = "s"]
pub struct S;

/// Alias `ah` for the attribute source, or the `attribute_history` hypertable.
#[derive(Iden, Clone, Copy)]
#[iden = "ah"]
pub struct Ah;

/// Alias `an` for the `attribute_names` table.
#[derive(Iden, Clone, Copy)]
#[iden = "an"]
pub struct An;

/// Alias `te` for the `temporal_entities` table.
#[derive(Iden, Clone, Copy)]
#[iden = "te"]
pub struct Te;

/// Alias `ahg` for the attribute rows the geo existential filters: the
/// `GeoProperty` instances within the temporal window (clause 5.7.4.4 S3), kept
/// distinct from [`Ah`] so the probe nested beside the window match cannot
/// capture its rows.
#[derive(Iden, Clone, Copy)]
#[iden = "ahg"]
pub struct Ahg;

/// Alias `rs` for the `temporal_attribute_streams` rows a candidate read
/// names the Evolutions of.
#[derive(Iden, Clone, Copy)]
#[iden = "rs"]
pub struct Rs;

/// Alias `rh` for the `attribute_history` rows a candidate read names the
/// Evolutions of.
#[derive(Iden, Clone, Copy)]
#[iden = "rh"]
pub struct Rh;

/// The `entity_id` name stage 1 projects the selected entity URI under.
#[derive(Iden, Clone, Copy)]
#[iden = "entity_id"]
pub struct EntityIdColumn;

#[cfg(test)]
mod tests {
    use crate::alias::{Ah, Ahg, An, EntityIdColumn, Rh, Rs, S, Te};
    use sea_query::Iden;

    #[test]
    fn table_aliases() {
        pretty_assertions::assert_eq!(S.to_string(), "s");
        pretty_assertions::assert_eq!(Ah.to_string(), "ah");
        pretty_assertions::assert_eq!(An.to_string(), "an");
        pretty_assertions::assert_eq!(Te.to_string(), "te");
        pretty_assertions::assert_eq!(Ahg.to_string(), "ahg");
        pretty_assertions::assert_eq!(Rs.to_string(), "rs");
        pretty_assertions::assert_eq!(Rh.to_string(), "rh");
        pretty_assertions::assert_eq!(EntityIdColumn.to_string(), "entity_id");
    }
}
