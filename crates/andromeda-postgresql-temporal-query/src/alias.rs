//! The table aliases and projection output name the temporal statement reads
//! its relations under.
//!
//! These are query-construction identifiers, not physical schema: they name how
//! stage 1 aliases the relations it selects from, and change when the statement
//! restructures its joins rather than when the database schema does. The
//! physical tables they alias live in `andromeda-postgresql-schema`.

use sea_query::Iden;

/// Alias `s` for the `temporal_attribute_streams` table.
#[derive(Iden, Clone, Copy)]
#[iden = "s"]
pub struct S;

/// Alias `ah` for the `attribute_history` hypertable.
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

/// Alias `ce` for the current-state `entities` row a `q` or geo predicate reads.
///
/// A `q` expression and a geo query filter current state (clauses 4.9 and 4.10)
/// and read relations - the JSONB payload and the default `location` columns -
/// that only the current-state relations have. Stage 1 reaches that row through
/// a correlated `EXISTS`, never a join, so an entity whose current state has
/// been deleted keeps its history queryable and simply matches no current-state
/// filter.
#[derive(Iden, Clone, Copy)]
#[iden = "ce"]
pub struct Ce;

/// Alias `cs` for the current-state `entity_search` row beside [`Ce`], joined
/// into the probe only when a geo query reads the default `location` columns.
#[derive(Iden, Clone, Copy)]
#[iden = "cs"]
pub struct Cs;

/// Alias `sg` for the `temporal_attribute_streams` row the geo existential
/// reads, kept distinct from [`S`] so the clause 5.7.4.4 geo probe nested beside
/// the main stream match cannot capture its rows.
#[derive(Iden, Clone, Copy)]
#[iden = "sg"]
pub struct Sg;

/// Alias `ang` for the `attribute_names` row the geo existential joins to name
/// the target `GeoProperty`.
#[derive(Iden, Clone, Copy)]
#[iden = "ang"]
pub struct Ang;

/// Alias `ahg` for the `attribute_history` rows the geo existential filters:
/// the `GeoProperty` instances within the temporal window (clause 5.7.4.4 S3).
#[derive(Iden, Clone, Copy)]
#[iden = "ahg"]
pub struct Ahg;

/// The `entity_id` name stage 1 projects the selected entity URI under.
#[derive(Iden, Clone, Copy)]
#[iden = "entity_id"]
pub struct EntityIdColumn;

#[cfg(test)]
mod tests {
    use crate::alias::{Ah, Ahg, An, Ang, Ce, Cs, EntityIdColumn, S, Sg, Te};
    use sea_query::Iden;

    #[test]
    fn table_aliases() {
        pretty_assertions::assert_eq!(S.to_string(), "s");
        pretty_assertions::assert_eq!(Ah.to_string(), "ah");
        pretty_assertions::assert_eq!(An.to_string(), "an");
        pretty_assertions::assert_eq!(Te.to_string(), "te");
        pretty_assertions::assert_eq!(Ce.to_string(), "ce");
        pretty_assertions::assert_eq!(Cs.to_string(), "cs");
        pretty_assertions::assert_eq!(Sg.to_string(), "sg");
        pretty_assertions::assert_eq!(Ang.to_string(), "ang");
        pretty_assertions::assert_eq!(Ahg.to_string(), "ahg");
        pretty_assertions::assert_eq!(EntityIdColumn.to_string(), "entity_id");
    }
}
