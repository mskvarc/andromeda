//! The check that a tombstone of history holds nothing beside its deletion,
//! which the upgrade adds again without `proof_seals`. The upgrade reads no
//! row to add it, and it refuses a tombstone holding a member all the same,
//! in rowstore history and in history the columnstore holds.

use crate::fixtures::{
    columnstore::{convert_history_to_columnstore, history_chunks_in_columnstore},
    current_state::insert_entity,
    history::{InstanceIdentity, insert_history_row, insert_stream},
    rehearsal::at_v1_0_0,
    statements::{insert, instant, name, smallint, uuid},
    test_error::UpgradeTestError,
};
use andromeda_postgresql_seaquery::statement::Statement;
use pretty_assertions::assert_eq;
use sea_query::{Expr, ExprTrait, Query, SimpleExpr};
use sqlx::PgPool;

/// The check a tombstone holding a member fails.
const TOMBSTONE_CHECK: &str = "chk_attribute_history_tombstone_members";

/// The instance of the observation v1.0.0 recorded.
const OBSERVED_INSTANCE: &str = "00000000-0000-4000-8000-0000000000e1";

/// The instance of the tombstone that holds nothing beside its deletion.
const BARE_TOMBSTONE: &str = "00000000-0000-4000-8000-0000000000e2";

/// The instance of the tombstone that holds an `expiresAt`.
const EXPIRING_TOMBSTONE: &str = "00000000-0000-4000-8000-0000000000e3";

/// Records a deletion of `instance` on `stream_key` in the chunk the seeded
/// observation is in, beside `members`.
async fn insert_tombstone(pool: &PgPool, stream_key: i64, instance: &str, members: Vec<(&str, SimpleExpr)>) -> Result<(), UpgradeTestError> {
    let mut row = vec![
        ("recorded_at", instant("2025-01-27T21:00:00Z")),
        ("write_id", uuid(instance)),
        ("stream_key", Expr::val(stream_key)),
        ("write_ordinal", Expr::val(0_i32)),
        ("attribute_kind", smallint(1)),
        ("value_kind", smallint(0)),
        ("deleted_at", instant("2025-01-27T21:00:00Z")),
        ("generated_instance_id", uuid(instance)),
    ];
    row.extend(members);
    insert(pool, "attribute_history", row).await
}

/// Whether each relation holding the tombstone check, history and its chunks,
/// holds it validated.
async fn tombstone_check_validations(pool: &PgPool) -> Result<Vec<bool>, UpgradeTestError> {
    let select = Query::select()
        .column(name("convalidated"))
        .from(name("pg_constraint"))
        .and_where(Expr::col(name("conname")).eq(TOMBSTONE_CHECK))
        .to_owned();
    Ok(select.to_query_scalar::<bool>().fetch_all(pool).await?)
}

/// Asserts that history takes a bare tombstone and refuses one holding an
/// `expiresAt`, by the tombstone check.
async fn assert_tombstone_members_are_refused(pool: &PgPool, stream_key: i64) -> Result<(), UpgradeTestError> {
    insert_tombstone(pool, stream_key, BARE_TOMBSTONE, Vec::new()).await?;
    let refused = insert_tombstone(pool, stream_key, EXPIRING_TOMBSTONE, vec![("expires_at", instant("2030-01-01T00:00:00Z"))]).await;
    assert!(
        matches!(&refused, Err(UpgradeTestError::Query(sqlx::Error::Database(error))) if error.constraint() == Some(TOMBSTONE_CHECK)),
        "{refused:?}"
    );
    Ok(())
}

/// A validated check would have read every row of history to be added. The
/// hypertable and the chunk v1.0.0 wrote hold it unvalidated, so none was
/// read.
#[tokio::test]
async fn the_upgrade_adds_the_tombstone_check_without_reading_history_and_it_refuses_a_tombstone_holding_a_member() -> Result<(), UpgradeTestError> {
    let (mut database, keys) = at_v1_0_0().await?;
    let entity_key = insert_entity(database.pool(), "urn:ngsi-ld:Store:001").await?;
    let stream = insert_stream(database.pool(), entity_key, keys.temperature).await?;
    insert_history_row(database.pool(), stream, &InstanceIdentity::Generated(OBSERVED_INSTANCE), &[]).await?;

    database.migrate_to_current().await?;

    assert_eq!(tombstone_check_validations(database.pool()).await?, [false, false]);
    assert_tombstone_members_are_refused(database.pool(), stream).await?;
    database.drop_database().await?;
    Ok(())
}

#[tokio::test]
async fn a_tombstone_holding_a_member_is_refused_in_history_the_columnstore_holds() -> Result<(), UpgradeTestError> {
    let (mut database, keys) = at_v1_0_0().await?;
    let entity_key = insert_entity(database.pool(), "urn:ngsi-ld:Store:001").await?;
    let stream = insert_stream(database.pool(), entity_key, keys.temperature).await?;
    insert_history_row(database.pool(), stream, &InstanceIdentity::Generated(OBSERVED_INSTANCE), &[]).await?;
    convert_history_to_columnstore(database.pool()).await?;
    assert_eq!(history_chunks_in_columnstore(database.pool()).await?, [true]);

    database.migrate_to_current().await?;

    assert_tombstone_members_are_refused(database.pool(), stream).await?;
    database.drop_database().await?;
    Ok(())
}
