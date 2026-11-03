//! The reified Scopes a Temporal Evolution is written with, refused and
//! admitted, shared by every write path that reads one.
//!
//! Clause 4.5.6 lets a reified Scope "only have the non-reified Temporal
//! Properties createdAt, modifiedAt, deletedAt and observedAt as
//! sub-Properties", so each refused case adds one other member to an
//! otherwise valid instance.

use andromeda_ngsi_ld_entity::{member_shape::MemberShape, scope_refusal::ScopeRefusal};
use andromeda_ngsi_ld_model::{request_body::RequestBody, temporal::written_temporal_entity::WrittenTemporalEntity};
use andromeda_ngsi_ld_representation::compact_node::context_reading::ContextReading;
use serde_json::{Value, json};

/// The Temporal Evolution of `urn:ngsi-ld:Vehicle:1` whose `scope` member is
/// `scope`, beside one `speed` observation.
pub(crate) fn evolution_with_scope(scope: &Value) -> WrittenTemporalEntity {
    let text = json!({
        "id": "urn:ngsi-ld:Vehicle:1",
        "type": "Vehicle",
        "scope": scope,
        "speed": [{"type": "Property", "value": 1, "observedAt": "2025-01-01T00:00:00Z"}]
    })
    .to_string();
    WrittenTemporalEntity::from_request_text(text.as_bytes(), ContextReading::keyword_spellings()).expect("a written evolution")
}

/// One reified Scope instance naming `/Madrid`, observed once, beside the
/// compact member `name` holding `value`.
fn madrid_with(name: &str, value: &Value) -> Value {
    let mut instance = json!({"type": "Property", "value": "/Madrid", "observedAt": "2025-01-01T00:00:00Z"});
    instance[name] = value.clone(); // clone: each case owns the member it adds
    json!([instance])
}

/// Each compact reified Scope a write refuses, with the refusal naming what
/// it carries beyond what clause 4.5.6 gives it.
pub(crate) fn refused_scopes() -> Vec<(Value, ScopeRefusal)> {
    vec![
        (
            madrid_with("datasetId", &json!("urn:ngsi-ld:Dataset:a")),
            ScopeRefusal::MemberForbidden {
                member: MemberShape::DatasetId,
            },
        ),
        (
            madrid_with("instanceId", &json!("urn:ngsi-ld:instance:1")),
            ScopeRefusal::MemberForbidden {
                member: MemberShape::InstanceId,
            },
        ),
        (
            madrid_with("expiresAt", &json!("2030-01-01T00:00:00Z")),
            ScopeRefusal::MemberForbidden {
                member: MemberShape::ExpiresAt,
            },
        ),
        (
            madrid_with("unitCode", &json!("CEL")),
            ScopeRefusal::MemberForbidden { member: MemberShape::UnitCode },
        ),
        (
            madrid_with("valueType", &json!("Text")),
            ScopeRefusal::MemberForbidden {
                member: MemberShape::ValueType,
            },
        ),
        (
            madrid_with("sensorNote", &json!({"type": "Property", "value": "north"})),
            ScopeRefusal::SubAttributeForbidden {
                attribute: "https://uri.etsi.org/ngsi-ld/default-context/sensorNote".parse().expect("valid attribute name"),
            },
        ),
        (
            json!([{"type": "Relationship", "object": "urn:ngsi-ld:Scope:Madrid", "observedAt": "2025-01-01T00:00:00Z"}]),
            ScopeRefusal::NotAProperty,
        ),
    ]
}

/// A reified Scope carrying every member clause 4.5.6 gives it: `/Madrid`
/// observed and system-timed, then its deletion.
pub(crate) fn admitted_scope() -> Value {
    json!([
        {
            "type": "Property",
            "value": "/Madrid",
            "observedAt": "2025-01-01T00:00:00Z",
            "createdAt": "2025-01-01T00:00:01Z",
            "modifiedAt": "2025-01-01T00:00:01Z"
        },
        {"type": "Property", "value": "urn:ngsi-ld:null", "observedAt": "2025-01-02T00:00:00Z", "deletedAt": "2025-01-02T00:00:00Z"}
    ])
}
