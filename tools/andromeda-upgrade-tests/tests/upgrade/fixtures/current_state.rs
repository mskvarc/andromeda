//! The rows of an Entity's current state as v1.0.0 wrote them: the
//! dictionaries, the Entity, its search row, its type membership and its
//! attribute instances.

use crate::fixtures::{
    statements::{composites, document, insert, insert_returning, instant, integers, numeric, smallint, smallints},
    test_error::UpgradeTestError,
};
use andromeda_postgresql_seaquery::pg_type::PostgresType;
use sea_query::{Expr, SimpleExpr};
use serde_json::Value;
use sqlx::PgPool;

/// The instant every seeded row was created and modified at.
const WRITTEN_AT: &str = "2025-01-27T21:00:00Z";

/// Adds `iri` to the attribute name dictionary and returns its key.
pub(crate) async fn intern_attribute(pool: &PgPool, iri: &str) -> Result<i32, UpgradeTestError> {
    insert_returning::<i32>(pool, "attribute_names", "attribute_name_key", vec![("name", Expr::val(iri))]).await
}

/// Adds `iri` to the entity type name dictionary and returns its key.
pub(crate) async fn intern_type(pool: &PgPool, iri: &str) -> Result<i32, UpgradeTestError> {
    insert_returning::<i32>(pool, "entity_type_names", "entity_type_name_key", vec![("name", Expr::val(iri))]).await
}

/// Inserts an Entity identified by `id` and returns its key.
pub(crate) async fn insert_entity(pool: &PgPool, id: &str) -> Result<i64, UpgradeTestError> {
    insert_returning::<i64>(
        pool,
        "entities",
        "entity_key",
        vec![("created_at", instant(WRITTEN_AT)), ("modified_at", instant(WRITTEN_AT)), ("id", Expr::val(id))],
    )
    .await
}

/// Inserts the search row of an Entity of `type_keys` holding one instance of
/// each attribute of `attribute_keys`, all of the Property family.
pub(crate) async fn insert_entity_search(pool: &PgPool, entity_key: i64, type_keys: &[i32], attribute_keys: &[i32]) -> Result<(), UpgradeTestError> {
    let kinds = vec![1_i16; attribute_keys.len()];
    insert(
        pool,
        "entity_search",
        vec![
            ("entity_key", Expr::val(entity_key)),
            ("entity_type_name_keys", integers(type_keys)),
            ("attribute_name_keys", integers(attribute_keys)),
            ("attribute_instance_name_keys", integers(attribute_keys)),
            ("attribute_instance_kinds", smallints(&kinds)),
        ],
    )
    .await
}

/// Records that the Entity `id` carries `type_key` in its current state and
/// its Temporal Evolution.
pub(crate) async fn insert_type_member(pool: &PgPool, entity_key: i64, type_key: i32, id: &str) -> Result<(), UpgradeTestError> {
    insert(
        pool,
        "entity_type_members",
        vec![
            ("entity_key", Expr::val(entity_key)),
            ("entity_type_name_key", Expr::val(type_key)),
            ("is_current_member", Expr::val(true)),
            ("is_evolution_member", Expr::val(true)),
            ("id", Expr::val(id)),
        ],
    )
    .await
}

/// Inserts a Property instance holding the number 21 beside `sub_attributes`.
pub(crate) async fn insert_number_attribute(
    pool: &PgPool,
    entity_key: i64,
    attribute_key: i32,
    ordinal: i32,
    sub_attributes: &[Value],
) -> Result<(), UpgradeTestError> {
    let mut row = instance_row(entity_key, attribute_key, ordinal);
    row.push(("value_kind", smallint(1)));
    row.push(("value_number", numeric("21")));
    if !sub_attributes.is_empty() {
        row.push(("sub_attributes", composites(PostgresType::SubAttribute, sub_attributes)));
    }
    insert(pool, "current_attributes", row).await
}

/// Inserts a Property instance whose value is the expanded object `object`,
/// carrying `seals` as v1.0.0 stored them beside the value.
pub(crate) async fn insert_object_attribute(
    pool: &PgPool,
    entity_key: i64,
    attribute_key: i32,
    ordinal: i32,
    object: &Value,
    seals: &Value,
) -> Result<(), UpgradeTestError> {
    let mut row = instance_row(entity_key, attribute_key, ordinal);
    row.push(("value_kind", smallint(5)));
    row.push(("value_object", document(object)));
    row.push(("proof_seals", document(seals)));
    insert(pool, "current_attributes", row).await
}

/// The columns every instance shares: its grain, its times and its family.
fn instance_row(entity_key: i64, attribute_key: i32, ordinal: i32) -> Vec<(&'static str, SimpleExpr)> {
    vec![
        ("entity_key", Expr::val(entity_key)),
        ("created_at", instant(WRITTEN_AT)),
        ("modified_at", instant(WRITTEN_AT)),
        ("attribute_name_key", Expr::val(attribute_key)),
        ("ordinal", Expr::val(ordinal)),
        ("attribute_kind", smallint(1)),
        ("dataset_key", Expr::val("")),
    ]
}
