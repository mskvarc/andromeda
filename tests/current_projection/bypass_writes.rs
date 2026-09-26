//! Rows written straight into the store, bypassing the broker.
//!
//! A schema constraint is only proven by a writer that does not go through the
//! Rust code enforcing the same rule first. These statements give such a
//! writer the parent rows a projection row needs, inside a transaction the
//! caller rolls back.

use serde_json::json;
use sqlx::PgConnection;

/// Insert a bare current Entity of one expanded type, with its document and
/// its search row, and return its surrogate key.
pub async fn insert_typed_entity(connection: &mut PgConnection, entity_id: &str, type_iri: &str) -> i64 {
    let payload = json!([{ "@id": entity_id, "@type": [type_iri] }]);
    let type_key: i32 = sqlx::query_scalar(
        "INSERT INTO entity_type_names (name) VALUES ($1) \
         ON CONFLICT (name) DO UPDATE SET name = EXCLUDED.name RETURNING entity_type_name_key",
    )
    .bind(type_iri)
    .fetch_one(&mut *connection)
    .await
    .expect("type dictionary insert must succeed");
    sqlx::query_scalar(
        "WITH ins AS (INSERT INTO entities (id) VALUES ($1) RETURNING entity_key), \
              doc AS (INSERT INTO entity_documents (entity_key, payload) SELECT entity_key, $3 FROM ins) \
         INSERT INTO entity_search (entity_key, entity_type_name_keys, attribute_name_keys) SELECT entity_key, ARRAY[$2], ARRAY[]::integer[] FROM ins RETURNING entity_key",
    )
    .bind(entity_id)
    .bind(type_key)
    .bind(&payload)
    .fetch_one(&mut *connection)
    .await
    .expect("entity insert must succeed")
}

/// Register one expanded attribute IRI in the attribute-name dictionary and
/// return its key, reusing the entry when it already exists.
pub async fn insert_attribute_name(connection: &mut PgConnection, attribute_iri: &str) -> i32 {
    sqlx::query_scalar(
        "INSERT INTO attribute_names (name) VALUES ($1) \
         ON CONFLICT (name) DO UPDATE SET name = EXCLUDED.name RETURNING attribute_name_key",
    )
    .bind(attribute_iri)
    .fetch_one(connection)
    .await
    .expect("the attribute dictionary must be writable")
}
