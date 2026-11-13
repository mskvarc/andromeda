//! Reading the upgraded rows back and holding them to the typed seal the
//! migrations produce.

use crate::fixtures::{
    statements::{name, read_documents},
    test_error::UpgradeTestError,
    vocabulary::{FURTHER_MEMBER, SEALED_ENTITY_ID},
};
use andromeda_postgresql_attribute_instance::{
    stored_proof::StoredProof,
    stored_value::StoredValue,
    sub_attribute::{SubAttributeElement, SubAttributeRecord},
};
use andromeda_postgresql_seaquery::{statement::Statement, structured_value::sub_attribute_value_object, unnest::unnest_of};
use pretty_assertions::assert_eq;
use sea_query::{Expr, ExprTrait, Order, Query};
use serde_json::{Value, json};
use sqlx::PgPool;

/// The `sub_attributes` of the one current-state row of `attribute_key`, as a
/// JSON array, or `None` when the row holds none.
pub(crate) async fn current_sub_attributes(pool: &PgPool, attribute_key: i32) -> Result<Option<Value>, UpgradeTestError> {
    let rows = read_documents(
        pool,
        "current_attributes",
        "sub_attributes",
        "attribute_name_key",
        Some(("attribute_name_key", i64::from(attribute_key))),
    )
    .await?;
    single(rows, "the current attribute")
}

/// The expanded proof node of every typed seal the current state holds, as
/// the model writes a structured value: the proof a typed seal stores,
/// rebuilt through the migrations' own `sub_attribute_value_object`.
pub(crate) async fn current_proofs_as_written(pool: &PgPool) -> Result<Vec<Value>, UpgradeTestError> {
    let field = |column: &str| Expr::col((name("seal"), name(column)));
    let select = Query::select()
        .expr(sub_attribute_value_object([field("value_kind"), field("value_object"), field("value_proof")]))
        .from(name("current_attributes"))
        .from_function(unnest_of(Expr::col((name("current_attributes"), name("sub_attributes")))), name("seal"))
        .to_owned();
    Ok(select.to_query_scalar::<Value>().fetch_all(pool).await?)
}

/// The seals of the current state as the broker reads them: every
/// sub-attribute is decoded through the broker's own `sub_attribute` record
/// and held to the invariants a stored element must meet, so an element the
/// broker could not serve fails the read.
pub(crate) async fn current_seals_as_the_broker_reads(pool: &PgPool) -> Result<Vec<StoredProof>, UpgradeTestError> {
    let select = Query::select()
        .column(name("sub_attributes"))
        .from(name("current_attributes"))
        .and_where(Expr::col(name("sub_attributes")).is_not_null())
        .order_by(name("entity_key"), Order::Asc)
        .to_owned();
    let rows = select.to_query_scalar::<Vec<SubAttributeRecord>>().fetch_all(pool).await?;
    let mut seals = Vec::new();
    for record in rows.into_iter().flatten() {
        if let StoredValue::Proof(proof) = SubAttributeElement::try_from(record)?.members.value {
            seals.push(*proof);
        }
    }
    Ok(seals)
}

/// The only document of `rows`, which holds one row.
pub(crate) fn single(rows: Vec<Option<Value>>, what: &'static str) -> Result<Option<Value>, UpgradeTestError> {
    let mut rows = rows.into_iter();
    let row = rows.next().ok_or(UpgradeTestError::Missing { what })?;
    assert_eq!(rows.next(), None, "{what} is one row");
    Ok(row)
}

/// The value of `field` in each element of the JSON array `elements`, as an
/// array.
pub(crate) fn field_of_each(elements: &Value, field: &str) -> Value {
    let values = elements
        .as_array()
        .map(|elements| elements.iter().map(|element| element[field].clone()).collect());
    Value::Array(values.unwrap_or_default())
}

/// Holds `element` to the typed seal the upgrade makes of the seal the
/// rehearsals seed: a `data-integrity-proof` of value kind 12 with no object
/// value, the two seals in fields of their own, and the proof's members typed.
pub(crate) fn assert_typed_seal(element: &Value, seal_key: i32, store_key: i32) {
    assert_eq!(element["attribute_name_key"], json!(seal_key));
    assert_eq!(element["attribute_kind"], json!(1));
    assert_eq!(element["value_kind"], json!(12));
    assert_eq!(element["value_object"], Value::Null);
    assert_eq!(element["seal_entity_id"], json!(SEALED_ENTITY_ID));
    assert_eq!(element["seal_entity_type_keys"], json!([store_key]));
    assert_eq!(element.get("proof_seals"), None);
    let proof = &element["value_proof"];
    assert_eq!(proof["created_digits"], json!(3));
    assert_eq!(proof["cryptosuite"], json!("eddsa-rdfc-2022"));
    assert_eq!(proof["proof_purpose"], json!("https://w3id.org/security#assertionMethod"));
    assert_eq!(proof["verification_method"], json!("https://example.org/keys/1#key-1"));
    assert_eq!(proof["proof_value"], json!("z58DAdFfa9SkqZMVPxAQp"));
    assert_eq!(proof["further_members"], json!({ FURTHER_MEMBER: [{"@value": "challenge-1"}] }));
}
