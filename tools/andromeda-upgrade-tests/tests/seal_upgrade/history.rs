//! The temporal rows of an Entity as v1.0.0 wrote them: one stream per
//! attribute grain, the history rows of a stream, and the claims a
//! client-supplied instance identifier takes.

use crate::{
    statements::{composites, insert, insert_returning, instant, numeric, smallint, uuid},
    test_error::UpgradeTestError,
};
use andromeda_postgresql_seaquery::pg_type::PostgresType;
use sea_query::Expr;
use serde_json::Value;
use sqlx::PgPool;

/// How the instance a history row records is identified.
pub(crate) enum InstanceIdentity<'text> {
    /// An identifier the broker minted, written as a UUID.
    Generated(&'text str),
    /// An identifier the client supplied, written as received.
    Verbatim(&'text str),
}

/// Inserts the stream of the default instance of `attribute_key` and returns
/// its key.
pub(crate) async fn insert_stream(pool: &PgPool, entity_key: i64, attribute_key: i32) -> Result<i64, UpgradeTestError> {
    insert_returning::<i64>(
        pool,
        "temporal_attribute_streams",
        "stream_key",
        vec![
            ("entity_key", Expr::val(entity_key)),
            ("attribute_name_key", Expr::val(attribute_key)),
            ("dataset_key", Expr::val("")),
        ],
    )
    .await
}

/// Records one observation of the number 21 on `stream_key`, beside
/// `sub_attributes`.
pub(crate) async fn insert_history_row(
    pool: &PgPool,
    stream_key: i64,
    identity: &InstanceIdentity<'_>,
    sub_attributes: &[Value],
) -> Result<(), UpgradeTestError> {
    let mut row = vec![
        ("recorded_at", instant("2025-01-27T21:00:00Z")),
        ("write_id", uuid("00000000-0000-4000-8000-0000000000a1")),
        ("stream_key", Expr::val(stream_key)),
        ("write_ordinal", Expr::val(0_i32)),
        ("attribute_kind", smallint(1)),
        ("value_kind", smallint(1)),
        ("observed_at", instant("2025-01-27T21:00:00Z")),
        ("value_number", numeric("21")),
    ];
    match identity {
        InstanceIdentity::Generated(instance) => row.push(("generated_instance_id", uuid(instance))),
        InstanceIdentity::Verbatim(instance) => row.push(("verbatim_instance_id", Expr::val(*instance))),
    }
    if !sub_attributes.is_empty() {
        row.push(("sub_attributes", composites(PostgresType::SubAttribute, sub_attributes)));
    }
    insert(pool, "attribute_history", row).await
}

/// Takes the claim a client-supplied instance identifier holds on
/// `attribute_key` of the Entity.
pub(crate) async fn insert_claim(pool: &PgPool, entity_key: i64, attribute_key: i32, instance: &str) -> Result<(), UpgradeTestError> {
    insert(
        pool,
        "attribute_history_instance_claims",
        vec![
            ("entity_key", Expr::val(entity_key)),
            ("attribute_name_key", Expr::val(attribute_key)),
            ("verbatim_instance_id", Expr::val(instance)),
        ],
    )
    .await
}
