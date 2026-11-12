//! The discovery and temporal census ledgers, which count instances and
//! streams as signed deltas that net out when a write removes them.

use crate::{
    statements::{insert, name, smallint},
    test_error::UpgradeTestError,
};
use andromeda_postgresql_seaquery::statement::Statement;
use sea_query::{Expr, Order, Query};
use sqlx::PgPool;
use std::collections::BTreeMap;

/// Counts one instance of `attribute_key`, as the write that stored it did.
pub(crate) async fn count_instance(pool: &PgPool, type_key: i32, attribute_key: i32) -> Result<(), UpgradeTestError> {
    insert(
        pool,
        "attribute_census",
        vec![("instance_delta", Expr::val(1_i64)), ("attribute_name_key", Expr::val(attribute_key))],
    )
    .await?;
    insert(
        pool,
        "entity_type_attribute_census",
        vec![
            ("instance_delta", Expr::val(1_i64)),
            ("entity_type_name_key", Expr::val(type_key)),
            ("attribute_name_key", Expr::val(attribute_key)),
            ("attribute_kind", smallint(1)),
        ],
    )
    .await
}

/// Counts one temporal stream of `attribute_key` under `type_key`.
pub(crate) async fn count_stream(pool: &PgPool, type_key: i32, attribute_key: i32) -> Result<(), UpgradeTestError> {
    insert(
        pool,
        "stream_type_census",
        vec![
            ("stream_delta", Expr::val(1_i64)),
            ("entity_type_name_key", Expr::val(type_key)),
            ("attribute_name_key", Expr::val(attribute_key)),
        ],
    )
    .await
}

/// The instances each attribute holds, per the ledger, attributes at zero left
/// out.
pub(crate) async fn instances_by_attribute(pool: &PgPool) -> Result<BTreeMap<i32, i64>, UpgradeTestError> {
    let select = Query::select()
        .columns([name("attribute_name_key"), name("instance_delta")])
        .from(name("attribute_census"))
        .order_by(name("attribute_name_key"), Order::Asc)
        .to_owned();
    let rows = select.to_query_as::<(i32, i64)>().fetch_all(pool).await?;
    Ok(net(rows.into_iter()))
}

/// The instances each attribute holds under each type, per the ledger, at zero
/// left out.
pub(crate) async fn instances_by_type_and_attribute(pool: &PgPool) -> Result<BTreeMap<(i32, i32), i64>, UpgradeTestError> {
    let select = Query::select()
        .columns([name("entity_type_name_key"), name("attribute_name_key"), name("instance_delta")])
        .from(name("entity_type_attribute_census"))
        .to_owned();
    let rows = select.to_query_as::<(i32, i32, i64)>().fetch_all(pool).await?;
    Ok(net(rows.into_iter().map(|(type_key, attribute, delta)| ((type_key, attribute), delta))))
}

/// The streams each attribute holds under each type, per the ledger, at zero
/// left out.
pub(crate) async fn streams_by_type_and_attribute(pool: &PgPool) -> Result<BTreeMap<(i32, i32), i64>, UpgradeTestError> {
    let select = Query::select()
        .columns([name("entity_type_name_key"), name("attribute_name_key"), name("stream_delta")])
        .from(name("stream_type_census"))
        .to_owned();
    let rows = select.to_query_as::<(i32, i32, i64)>().fetch_all(pool).await?;
    Ok(net(rows.into_iter().map(|(type_key, attribute, delta)| ((type_key, attribute), delta))))
}

/// The deltas summed per key, keys that net to zero dropped.
fn net<Key: Ord>(deltas: impl Iterator<Item = (Key, i64)>) -> BTreeMap<Key, i64> {
    let mut totals = BTreeMap::new();
    for (key, delta) in deltas {
        *totals.entry(key).or_insert(0) += delta;
    }
    totals.retain(|_, total| *total != 0);
    totals
}
