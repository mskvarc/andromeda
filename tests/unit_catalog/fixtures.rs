//! Entity bodies and persistence lookups shared by the unit-catalogue suite.

use andromeda_test_fixtures::context::CORE_CONTEXT;
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
    sqlx::query(
        "SELECT ah.unit_code_id \
         FROM attribute_history ah \
         JOIN temporal_attribute_streams s ON s.stream_key = ah.stream_key \
         JOIN temporal_entities te ON te.temporal_entity_key = s.temporal_entity_key \
         JOIN attribute_names an ON an.attribute_name_key = s.attribute_name_key \
         WHERE te.id = $1 AND an.name = $2 \
         ORDER BY ah.recorded_at, ah.write_ordinal",
    )
    .bind(entity_id)
    .bind(attribute_iri)
    .fetch_all(pool)
    .await
    .expect("history rows must be readable")
    .into_iter()
    .map(|row| row.try_get::<Option<i16>, _>("unit_code_id").expect("the unit_code_id column decodes"))
    .collect()
}

/// The number of sub-attribute elements of every history row of one entity's
/// attribute, `None` for a row without any.
pub async fn history_sub_attribute_counts(pool: &PgPool, entity_id: &str, attribute_iri: &str) -> Vec<Option<i32>> {
    sqlx::query(
        "SELECT cardinality(ah.sub_attributes) AS elements \
         FROM attribute_history ah \
         JOIN temporal_attribute_streams s ON s.stream_key = ah.stream_key \
         JOIN temporal_entities te ON te.temporal_entity_key = s.temporal_entity_key \
         JOIN attribute_names an ON an.attribute_name_key = s.attribute_name_key \
         WHERE te.id = $1 AND an.name = $2 \
         ORDER BY ah.recorded_at, ah.write_ordinal",
    )
    .bind(entity_id)
    .bind(attribute_iri)
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
