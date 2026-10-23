//! Entity bodies and persistence lookups shared by the unit-catalogue suite.

use andromeda_postgresql_schema::{
    current::Entities,
    name_dictionary::AttributeNames,
    temporal::{AttributeHistory, TemporalAttributeStreams},
};
use andromeda_postgresql_seaquery::{pg_array::cardinality, statement::Statement};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use sea_query::{Alias, Expr, ExprTrait, Order, Query, SelectStatement};
use serde_json::{Value, json};
use sqlx::{PgPool, Row};

/// The suite name every environment in this suite is allocated under.
pub const SUITE: &str = "unit_catalog";

/// A Property carrying `unitCode` and a sub-attribute carrying its own.
///
/// Both levels are present because clause 5.2.5 attaches the member to any
/// Property, including one nested as a sub-attribute, and only the instance's
/// own member is the one the column takes over.
pub fn device_with_unit(id: &str, code: &str, sub_attribute_code: &str) -> Value {
    json!({
        "id": id,
        "type": "Device",
        "weight": {
            "type": "Property",
            "value": 5,
            "unitCode": code,
            "accuracy": {"type": "Property", "value": 0.1, "unitCode": sub_attribute_code}
        },
        "serial": {"type": "Property", "value": "sn-1"},
        "@context": [CORE_CONTEXT]
    })
}

/// An attribute of the named family carrying a `unitCode` member.
///
/// The families here are the ones clauses 4.5.3, 4.5.18, 4.5.20 and 4.5.24
/// define without a unit, so every body this builds is one the broker must
/// refuse.
pub fn entity_with_unitless_attribute(id: &str, attribute: &Value) -> Value {
    json!({
        "id": id,
        "type": "Device",
        "member": attribute,
        "@context": [CORE_CONTEXT]
    })
}

/// The `unit_code_id` recorded on every history row of one entity's attribute.
pub async fn history_unit_identities(pool: &PgPool, entity_id: &str, attribute_iri: &str) -> Vec<Option<i16>> {
    attribute_history_of(entity_id, attribute_iri)
        .column(AttributeHistory::UnitCodeId)
        .to_query()
        .fetch_all(pool)
        .await
        .expect("history rows must be readable")
        .into_iter()
        .map(|row| row.try_get::<Option<i16>, _>("unit_code_id").expect("the unit_code_id column decodes"))
        .collect()
}

/// The history rows of one entity's attribute in recording order, before any
/// column is selected.
fn attribute_history_of(entity_id: &str, attribute_iri: &str) -> SelectStatement {
    Query::select()
        .from(AttributeHistory::Table)
        .inner_join(
            TemporalAttributeStreams::Table,
            Expr::col((TemporalAttributeStreams::Table, TemporalAttributeStreams::StreamKey)).equals((AttributeHistory::Table, AttributeHistory::StreamKey)),
        )
        .inner_join(
            Entities::Table,
            Expr::col((Entities::Table, Entities::EntityKey)).equals((TemporalAttributeStreams::Table, TemporalAttributeStreams::EntityKey)),
        )
        .inner_join(
            AttributeNames::Table,
            Expr::col((AttributeNames::Table, AttributeNames::AttributeNameKey))
                .equals((TemporalAttributeStreams::Table, TemporalAttributeStreams::AttributeNameKey)),
        )
        .and_where(Expr::col((Entities::Table, Entities::Id)).eq(entity_id))
        .and_where(Expr::col((AttributeNames::Table, AttributeNames::Name)).eq(attribute_iri))
        .order_by((AttributeHistory::Table, AttributeHistory::RecordedAt), Order::Asc)
        .order_by((AttributeHistory::Table, AttributeHistory::WriteOrdinal), Order::Asc)
        .to_owned()
}

/// The number of sub-attribute elements of every history row of one entity's
/// attribute, `None` for a row without any.
pub async fn history_sub_attribute_counts(pool: &PgPool, entity_id: &str, attribute_iri: &str) -> Vec<Option<i32>> {
    attribute_history_of(entity_id, attribute_iri)
        .expr_as(
            cardinality(Expr::col((AttributeHistory::Table, AttributeHistory::SubAttributes))),
            Alias::new("elements"),
        )
        .to_query()
        .fetch_all(pool)
        .await
        .expect("history rows must be readable")
        .into_iter()
        .map(|row| row.try_get::<Option<i32>, _>("elements").expect("the elements column decodes"))
        .collect()
}

/// The expanded IRI of an attribute written under the core context's default vocabulary.
pub fn default_context_iri(term: &str) -> String {
    format!("https://uri.etsi.org/ngsi-ld/default-context/{term}")
}
