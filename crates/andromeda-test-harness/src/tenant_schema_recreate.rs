//! Simulating an out-of-band tenant schema drop and recreate.
//!
//! An operator can drop and recreate a tenant's schema directly against the
//! database while the broker keeps the tenant's runtime alive, its in-memory
//! `NameCache` included. A recreate resets the dictionary's `IDENTITY`
//! sequences, so a cached name-to-key mapping points at a dictionary row
//! that no longer exists. This helper reproduces that exact signature on a
//! tenant's own database pool: it resets the dictionary and re-stamps the
//! schema, which is what the broker's schema-epoch check detects.
//!
//! The global reset SQL lives here in the harness rather than inline in an
//! integration test, so the test-infrastructure boundary that forbids ad-hoc
//! `TRUNCATE` in `tests/` stays intact.

use crate::database_reset::DatabaseResetError;
use sqlx::PgPool;

/// The statements of [`simulate_out_of_band_recreate`], run in order in one
/// transaction.
///
/// The attribute dictionary refuses a truncate through a statement trigger,
/// because the broker's schema holds its keys where no foreign key reaches. An
/// operator recreating the schema acts as its owner, outside every rule that
/// schema keeps, so the helper lifts that one guard for its own truncate and
/// restores it before the transaction commits; no other session ever sees the
/// dictionary unguarded.
const OUT_OF_BAND_RECREATE_STATEMENTS: [&str; 4] = [
    "ALTER TABLE attribute_names DISABLE TRIGGER trg_attribute_names_append_only_truncate",
    "TRUNCATE attribute_names RESTART IDENTITY CASCADE",
    "ALTER TABLE attribute_names ENABLE TRIGGER trg_attribute_names_append_only_truncate",
    "UPDATE schema_metadata SET applied_at = clock_timestamp()",
];

/// Reset the attribute-name dictionary and advance the schema stamp on a
/// tenant's database, the signature an out-of-band schema recreate leaves.
///
/// `TRUNCATE ... RESTART IDENTITY CASCADE` empties the dictionary and restarts
/// its `IDENTITY`, cascading to the projection and temporal rows that reference
/// it, so a re-minted name takes a fresh key; advancing `schema_metadata`'s
/// `applied_at` to the current instant is the stamp change the broker reads as
/// a recreate.
///
/// # Errors
///
/// Returns [`DatabaseResetError::Sqlx`] when a statement fails, in which case
/// the transaction rolls back and the dictionary keeps its guard.
pub async fn simulate_out_of_band_recreate(pool: &PgPool) -> Result<(), DatabaseResetError> {
    let mut transaction = pool.begin().await.map_err(DatabaseResetError::from_sqlx)?;
    for statement in OUT_OF_BAND_RECREATE_STATEMENTS {
        sqlx::query(statement).execute(&mut *transaction).await.map_err(DatabaseResetError::from_sqlx)?;
    }
    transaction.commit().await.map_err(DatabaseResetError::from_sqlx)
}

#[cfg(test)]
mod tests {
    use crate::tenant_schema_recreate::OUT_OF_BAND_RECREATE_STATEMENTS;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_out_of_band_recreate_disables_only_the_truncate_guard_inside_its_transaction() {
        assert_eq!(
            OUT_OF_BAND_RECREATE_STATEMENTS,
            [
                "ALTER TABLE attribute_names DISABLE TRIGGER trg_attribute_names_append_only_truncate",
                "TRUNCATE attribute_names RESTART IDENTITY CASCADE",
                "ALTER TABLE attribute_names ENABLE TRIGGER trg_attribute_names_append_only_truncate",
                "UPDATE schema_metadata SET applied_at = clock_timestamp()",
            ]
        );
        assert!(
            !OUT_OF_BAND_RECREATE_STATEMENTS.iter().any(|statement| statement.contains("DISABLE TRIGGER ALL")
                || statement.contains("trg_attribute_names_append_only_row")
                || statement.contains("entity_type_names")),
            "only the attribute dictionary's truncate guard is lifted"
        );
    }
}
