//! A context-source subscription row holds the `datasetId` its client wrote
//! once the upgrade adds the check on context-source rows again without
//! `dataset_ids`, and the check still holds `local_only` at its default. The
//! check is added without reading a row, and the rows v1.0.0 stored stay.

use crate::fixtures::{
    rehearsal::at_v1_0_0,
    statements::{insert, instant, json_document, name, read_column, smallint, texts},
    test_error::UpgradeTestError,
};
use andromeda_postgresql_seaquery::statement::Statement;
use pretty_assertions::assert_eq;
use sea_query::{Expr, ExprTrait, Query, SimpleExpr};
use serde_json::json;
use sqlx::PgPool;

/// The check on context-source subscription rows.
const CSOURCE_CHECK: &str = "chk_subscriptions_csource_defaults";

/// Inserts the context-source subscription `id`, beside `members`.
async fn insert_csource_subscription(pool: &PgPool, id: &str, members: Vec<(&str, SimpleExpr)>) -> Result<(), UpgradeTestError> {
    let mut row = vec![
        ("id", Expr::val(id)),
        ("kind", smallint(2)),
        ("status", smallint(1)),
        ("context_source_revision", Expr::val(0_i64)),
        ("created_at", instant("2025-01-27T21:00:00Z")),
        ("modified_at", instant("2025-01-27T21:00:00Z")),
        ("active_since", instant("2025-01-27T21:00:00Z")),
        ("selected_since", instant("2025-01-27T21:00:00Z")),
        (
            "notification",
            json_document(&json!({"endpoint": {"uri": "https://receiver.example.com/notify"}})),
        ),
    ];
    row.extend(members);
    insert(pool, "subscriptions", row).await
}

/// Whether the insert `inserted` was refused by the check on context-source
/// rows.
fn refused_by_the_csource_check(inserted: &Result<(), UpgradeTestError>) -> bool {
    matches!(inserted, Err(UpgradeTestError::Query(sqlx::Error::Database(error))) if error.constraint() == Some(CSOURCE_CHECK))
}

/// Whether each relation holding the check on context-source rows holds it
/// validated.
async fn csource_check_validations(pool: &PgPool) -> Result<Vec<bool>, UpgradeTestError> {
    let select = Query::select()
        .column(name("convalidated"))
        .from(name("pg_constraint"))
        .and_where(Expr::col(name("conname")).eq(CSOURCE_CHECK))
        .to_owned();
    Ok(select.to_query_scalar::<bool>().fetch_all(pool).await?)
}

#[tokio::test]
async fn a_context_source_subscription_row_holds_its_dataset_ids_and_still_no_local_only() -> Result<(), UpgradeTestError> {
    let (mut database, _) = at_v1_0_0().await?;
    insert_csource_subscription(database.pool(), "urn:ngsi-ld:Subscription:stored", Vec::new()).await?;
    let refused_by_v1_0_0 = insert_csource_subscription(
        database.pool(),
        "urn:ngsi-ld:Subscription:refused",
        vec![("dataset_ids", texts(&[Some("urn:ngsi-ld:Dataset:a")]))],
    )
    .await;
    assert!(refused_by_the_csource_check(&refused_by_v1_0_0), "{refused_by_v1_0_0:?}");

    database.migrate_to_current().await?;

    let validations = csource_check_validations(database.pool()).await?;
    assert!(!validations.is_empty() && validations.iter().all(|validated| !validated), "{validations:?}");
    insert_csource_subscription(
        database.pool(),
        "urn:ngsi-ld:Subscription:dataset",
        vec![("dataset_ids", texts(&[Some("urn:ngsi-ld:Dataset:a"), Some("@none")]))],
    )
    .await?;
    let local_only = insert_csource_subscription(database.pool(), "urn:ngsi-ld:Subscription:local", vec![("local_only", Expr::val(true))]).await;
    assert!(refused_by_the_csource_check(&local_only), "{local_only:?}");
    assert_eq!(
        read_column::<String>(database.pool(), "subscriptions", "id").await?,
        ["urn:ngsi-ld:Subscription:dataset", "urn:ngsi-ld:Subscription:stored"]
    );
    database.drop_database().await?;
    Ok(())
}
