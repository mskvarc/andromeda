//! Transaction-scoped temporary copies of a relation's shape.
//!
//! sea-query has no form for `CREATE TEMPORARY TABLE ... (LIKE ...)` nor for
//! `ON COMMIT DROP`, so the statement is spelled here, its names quoted through
//! the same idens every other statement names the relations by.

use sea_query::{DynIden, Iden};

/// The schema PostgreSQL resolves a session's temporary relations in, which
/// a statement names to read a temporary relation rather than the persistent
/// one of the same name.
#[derive(Iden, Clone, Copy)]
#[iden = "pg_temp"]
pub struct TemporarySchema;

/// `CREATE TEMPORARY TABLE "<name>" (LIKE "<schema>"."<source>" INCLUDING
/// DEFAULTS INCLUDING INDEXES) ON COMMIT DROP`: an empty relation of the
/// columns, defaults and indexes of `source`, gone when the transaction that
/// created it ends.
///
/// The indexes come along so a statement written against `source` -- an
/// `ON CONFLICT` naming its unique index included -- runs unchanged against
/// the copy. Constraints, identities and triggers do not: the copy holds rows
/// the caller places, keys included.
#[must_use]
pub fn temporary_copy_of(name: &DynIden, schema: &DynIden, source: &DynIden) -> String {
    format!("CREATE TEMPORARY TABLE \"{name}\" (LIKE \"{schema}\".\"{source}\" INCLUDING DEFAULTS INCLUDING INDEXES) ON COMMIT DROP")
}

#[cfg(test)]
mod tests {
    use crate::temporary_table::{TemporarySchema, temporary_copy_of};
    use sea_query::{Alias, Iden, IntoIden};

    #[test]
    fn the_copy_is_temporary_and_names_its_source_qualified() {
        pretty_assertions::assert_eq!(
            temporary_copy_of(
                &Alias::new("temporal_entities").into_iden(),
                &Alias::new("public").into_iden(),
                &Alias::new("temporal_entities").into_iden()
            ),
            r#"CREATE TEMPORARY TABLE "temporal_entities" (LIKE "public"."temporal_entities" INCLUDING DEFAULTS INCLUDING INDEXES) ON COMMIT DROP"#
        );
        pretty_assertions::assert_eq!(TemporarySchema.to_string(), "pg_temp");
    }
}
