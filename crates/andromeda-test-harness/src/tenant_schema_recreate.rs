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
//! The global reset lives here in the harness rather than inline in an
//! integration test, so the test-infrastructure boundary that forbids ad-hoc
//! `TRUNCATE` in `tests/` stays intact.

use crate::database_reset::DatabaseResetError;
use andromeda_postgresql_schema::{name_dictionary::AttributeNames, tenancy::SchemaMetadata};
use andromeda_postgresql_seaquery::{
    clock::wall_clock,
    command::Command,
    relation_truncation::RelationTruncation,
    statement::Statement,
    trigger_switch::{TriggerState, TriggerSwitch},
};
use sea_query::{Alias, Query, TableName, UpdateStatement};
use sqlx::PgPool;

/// The statement trigger by which the attribute dictionary refuses a truncate,
/// because the broker's schema holds its keys where no foreign key reaches.
const TRUNCATE_GUARD: &str = "trg_attribute_names_append_only_truncate";

/// The guard of the attribute dictionary's truncate, switched to `state`.
///
/// An operator recreating the schema acts as its owner, outside every rule that
/// schema keeps, so the helper lifts that one guard for its own truncate and
/// restores it before the transaction commits; no other session ever sees the
/// dictionary unguarded.
fn truncate_guard(state: TriggerState) -> TriggerSwitch {
    TriggerSwitch::new(TableName::from(AttributeNames::Table), Alias::new(TRUNCATE_GUARD), state)
}

/// The truncation of the attribute dictionary, its identity restarted and its
/// referencing rows reclaimed.
fn dictionary_truncation() -> RelationTruncation {
    RelationTruncation::new(vec![TableName::from(AttributeNames::Table)])
}

/// The new stamp: the schema contract's `applied_at` advanced to the current
/// instant.
fn stamp_advance() -> UpdateStatement {
    Query::update()
        .table(SchemaMetadata::Table)
        .value(SchemaMetadata::AppliedAt, wall_clock())
        .to_owned()
}

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
    truncate_guard(TriggerState::Disabled)
        .to_query()
        .execute(&mut *transaction)
        .await
        .map_err(DatabaseResetError::from_sqlx)?;
    dictionary_truncation()
        .to_query()
        .execute(&mut *transaction)
        .await
        .map_err(DatabaseResetError::from_sqlx)?;
    truncate_guard(TriggerState::Enabled)
        .to_query()
        .execute(&mut *transaction)
        .await
        .map_err(DatabaseResetError::from_sqlx)?;
    stamp_advance()
        .to_query()
        .execute(&mut *transaction)
        .await
        .map_err(DatabaseResetError::from_sqlx)?;
    transaction.commit().await.map_err(DatabaseResetError::from_sqlx)
}

#[cfg(test)]
mod tests {
    use crate::tenant_schema_recreate::{dictionary_truncation, stamp_advance, truncate_guard};
    use andromeda_postgresql_seaquery::{
        relation_truncation::RelationTruncation,
        trigger_switch::{TriggerState, TriggerSwitch},
    };
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, PostgresQueryBuilder, TableName};

    #[test]
    fn the_out_of_band_recreate_lifts_only_the_attribute_dictionary_truncate_guard() {
        let attribute_names = || TableName::from(Alias::new("attribute_names"));
        assert_eq!(
            truncate_guard(TriggerState::Disabled),
            TriggerSwitch::new(
                attribute_names(),
                Alias::new("trg_attribute_names_append_only_truncate"),
                TriggerState::Disabled
            )
        );
        assert_eq!(
            truncate_guard(TriggerState::Enabled),
            TriggerSwitch::new(attribute_names(), Alias::new("trg_attribute_names_append_only_truncate"), TriggerState::Enabled)
        );
        assert_eq!(dictionary_truncation(), RelationTruncation::new(vec![attribute_names()]));
    }

    #[test]
    fn the_stamp_advances_to_the_wall_clock() {
        assert_eq!(
            stamp_advance().to_string(PostgresQueryBuilder),
            r#"UPDATE "schema_metadata" SET "applied_at" = CLOCK_TIMESTAMP()"#
        );
    }
}
