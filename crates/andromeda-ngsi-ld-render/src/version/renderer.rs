//! Driving the clause 4.3.6.8 fallback tables over a rendered payload.

use crate::version::{
    adaptation::VersionAdaptation,
    adapted_payload::AdaptedPayload,
    reified_attribute::adapt_reified_attribute,
    simplified_attribute::adapt_simplified_attribute,
    simplified_temporal_attribute::adapt_simplified_temporal_attribute,
};
use andromeda_ngsi_ld_lexicon::spec_version::SpecVersion;
use andromeda_ngsi_ld_representation::{entity_members::ENTITY_MEMBERS, response_format::ResponseFormat};
use serde_json::Value;

/// Adapt a serialized NGSI-LD entity payload to conform to an older spec version.
///
/// Applies the fallback tables from clause 4.3.6.8:
/// - Table 4.3.6.8-1: entity-level removals
/// - Table 4.3.6.8-2: property-level removals
/// - Table 4.3.6.8-3: relationship-level removals
///
/// Handles both single entities (JSON objects) and entity arrays.
///
/// The representation the payload is already rendered in decides how an
/// attribute is read: clause 4.5.1 reifies every attribute into an object
/// naming its `type`, clause 4.5.2.3 drops that `type` and leaves the family
/// to be inferred from the value member, and clause 4.5.4 reduces the whole
/// attribute to its bare value. The default is the normalized reading; set it
/// with [`VersionRenderer::with_format`] when the payload has already been
/// rendered in one of the other two. Reading a clause 4.5.4 payload under the
/// normalized reading is not a near miss: the wrapper key that clause carries
/// -- `languageMap`, `json` or `vocab` -- would be taken for an instance's
/// value member and renamed to `value`, producing an attribute whose value is a
/// `{"value": ...}` object that clause 4.5.4 defines for nothing.
///
/// Which NGSI-LD Element the payload is decides two further readings; the
/// default is an Entity, and [`VersionRenderer::with_adapted_payload`] states
/// otherwise. See [`AdaptedPayload`].
#[derive(Debug)]
pub struct VersionRenderer {
    /// The spec version the payload is adapted down to.
    target: SpecVersion,
    /// The representation the payload is already rendered in.
    format: ResponseFormat,
    /// Which NGSI-LD Element the payload is.
    adapted_payload: AdaptedPayload,
    /// The payload being adapted.
    payload: Value,
}

impl VersionRenderer {
    /// Create a renderer targeting the given NGSI-LD version with an empty payload.
    #[must_use]
    pub const fn new(target: SpecVersion) -> VersionRenderer {
        VersionRenderer {
            target,
            format: ResponseFormat::Normalized,
            adapted_payload: AdaptedPayload::Entity,
            payload: Value::Null,
        }
    }

    /// Set the representation the payload is already rendered in.
    #[must_use]
    pub const fn with_format(mut self, format: ResponseFormat) -> VersionRenderer {
        self.format = format;
        self
    }

    /// Set which NGSI-LD Element the payload is, which decides how an
    /// attribute's JSON array and its `observedAt` are read.
    #[must_use]
    pub const fn with_adapted_payload(mut self, adapted_payload: AdaptedPayload) -> VersionRenderer {
        self.adapted_payload = adapted_payload;
        self
    }

    /// Set the payload value to adapt.
    #[must_use]
    pub fn with_payload(mut self, payload: Value) -> VersionRenderer {
        self.payload = payload;
        self
    }

    /// Apply the clause 4.3.6.8 fallback tables and return the adapted payload
    /// together with whether adaptation was performed.
    ///
    /// The [`VersionAdaptation`] the caller receives drives the status code: a
    /// requested version that is the current one or newer needs no adaptation,
    /// so the payload is returned unchanged with
    /// [`VersionAdaptation::Unchanged`] and the response stays 200. Only a
    /// target below the conformant version runs the fallback tables, which
    /// makes the representation non-authoritative (203). Removing members
    /// cannot fail, so this answers with a value rather than a `Result` no
    /// caller could act on.
    #[must_use]
    pub fn render(self) -> (Value, VersionAdaptation) {
        let VersionRenderer {
            target,
            format,
            adapted_payload,
            mut payload,
        } = self;
        if target >= SpecVersion::CONFORMANT {
            return (payload, VersionAdaptation::Unchanged);
        }

        match &mut payload {
            Value::Object(_) => adapt_entity(&mut payload, target, format, adapted_payload),
            Value::Array(items) => {
                for item in items {
                    adapt_entity(item, target, format, adapted_payload);
                }
            }
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
        }
        (payload, VersionAdaptation::Applied { version: target })
    }
}

/// Adapt a single entity object.
fn adapt_entity(entity: &mut Value, target: SpecVersion, format: ResponseFormat, adapted_payload: AdaptedPayload) {
    let Some(obj) = entity.as_object_mut() else {
        return;
    };

    // Table 4.3.6.8-1: entity-level removals
    if target < SpecVersion::V1_9 {
        obj.remove("expiresAt");
    }
    if target < SpecVersion::V1_4 {
        obj.remove("scope");
    }
    // Table 4.3.6.8-1 note 1: a multi-valued `type` exists from 1.3 onwards;
    // an older target receives the first value alone.
    if target < SpecVersion::V1_3
        && let Some(Value::Array(types)) = obj.get_mut("type")
    {
        let first = types.first().cloned().unwrap_or(Value::Null); // clone: the first element outlives the array it is lifted out of
        obj.insert("type".to_string(), first);
    }

    // Collect attribute keys; clause 5.2.4's own members are not attributes and
    // are read from the one table every renderer shares.
    let attr_keys: Vec<String> = obj.keys().filter(|key| !ENTITY_MEMBERS.contains(&key.as_str())).cloned().collect();

    for key in attr_keys {
        if let Some(attr_value) = obj.get_mut(&key) {
            adapt_attribute(attr_value, target, format, adapted_payload);
        }
    }
}

/// Adapt one attribute of an entity, in the representation the payload carries.
///
/// A clause 4.5.9 or 4.5.19 temporal payload takes no `format` at all: clause
/// 6.3.12 selects it instead of the clause 4.5.1 representations, and its
/// attribute objects hold the plural members that representation defines rather
/// than an instance's value member.
fn adapt_attribute(value: &mut Value, target: SpecVersion, format: ResponseFormat, adapted_payload: AdaptedPayload) {
    if adapted_payload == AdaptedPayload::SimplifiedTemporalEvolution {
        adapt_simplified_temporal_attribute(value, target, adapted_payload);
        return;
    }
    match format {
        // Concise is the normalized shape with the inferrable `type` dropped
        // (clause 4.5.2.3), so one walk serves both; which member the family is
        // read from is what differs, and that is decided per instance.
        ResponseFormat::Normalized | ResponseFormat::Concise => adapt_reified_attribute(value, target, format, adapted_payload),
        ResponseFormat::KeyValues | ResponseFormat::Simplified => adapt_simplified_attribute(value, target),
    }
}

#[cfg(test)]
mod tests {
    use crate::version::{adaptation::VersionAdaptation, renderer::VersionRenderer};
    use andromeda_ngsi_ld_lexicon::spec_version::SpecVersion;
    use andromeda_ngsi_ld_representation::response_format::ResponseFormat;
    use serde_json::{Value, json};

    /// Adapt a whole payload in the normalized representation.
    fn adapt(target: SpecVersion, value: Value) -> Value {
        VersionRenderer::new(target).with_payload(value).render().0
    }

    /// Adapt a whole payload in the representation it was rendered in.
    fn adapt_as(target: SpecVersion, format: ResponseFormat, value: Value) -> Value {
        VersionRenderer::new(target).with_format(format).with_payload(value).render().0
    }

    #[test]
    fn a_conformant_or_newer_target_reports_no_adaptation() {
        // Clause 4.3.6.8: the payload is already authoritative for the current
        // version or newer, so no adaptation is performed and the response is
        // 200, not 203. This is what the short-circuit must report.
        let payload = json!({"id": "urn:x", "type": "T", "expiresAt": "2025-01-01T00:00:00Z"});
        let (adapted, adaptation) = VersionRenderer::new(SpecVersion::CONFORMANT).with_payload(payload.clone()).render();
        pretty_assertions::assert_eq!(adaptation, VersionAdaptation::Unchanged);
        pretty_assertions::assert_eq!(adapted, payload);
    }

    #[test]
    fn a_target_below_the_conformant_version_reports_adaptation_to_that_target() {
        let payload = json!({"id": "urn:x", "type": "T", "expiresAt": "2025-01-01T00:00:00Z"});
        let (_, adaptation) = VersionRenderer::new(SpecVersion::V1_4).with_payload(payload).render();
        pretty_assertions::assert_eq!(adaptation, VersionAdaptation::Applied { version: SpecVersion::V1_4 });
    }

    #[test]
    fn no_adaptation_for_conformant_version() {
        let value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": "TestEntity",
            "expiresAt": "2025-01-01T00:00:00Z",
            "scope": ["/A"],
            "temperature": {
                "type": "Property",
                "value": 25.0,
                "datasetId": "urn:ds:1",
                "expiresAt": "2025-01-01T00:00:00Z"
            }
        });
        let original = value.clone();
        let adapted = adapt(SpecVersion::V1_9, value);
        pretty_assertions::assert_eq!(adapted, original);
    }

    /// Table 4.3.6.8-1: entity `expiresAt` arrived at 1.9 and `scope` at 1.4.
    #[test]
    fn the_entity_members_that_postdate_the_target_are_removed() {
        let value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": "TestEntity",
            "expiresAt": "2025-01-01T00:00:00Z",
            "scope": ["/A/B"],
            "temperature": {"type": "Property", "value": 25.0}
        });
        let at_1_8 = adapt(SpecVersion::V1_8, value.clone());
        assert!(at_1_8.get("expiresAt").is_none(), "{at_1_8}");
        assert!(at_1_8.get("scope").is_some(), "1.8 knew the scope member: {at_1_8}");

        let at_1_3 = adapt(SpecVersion::V1_3, value);
        assert!(at_1_3.get("scope").is_none(), "{at_1_3}");
    }

    /// Table 4.3.6.8-1 note 1: a v1.0 target receives a single `type`, the
    /// first of the values the entity declared.
    #[test]
    fn v1_0_multi_valued_type_collapses_to_its_first_value() {
        let value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": ["Vehicle", "Car"],
            "speed": {"type": "Property", "value": 10}
        });
        let adapted = adapt(SpecVersion::V1_0, value);
        pretty_assertions::assert_eq!(adapted.get("type"), Some(&json!("Vehicle")));

        let kept = adapt(SpecVersion::V1_3, json!({"id": "urn:ngsi-ld:Entity:1", "type": ["Vehicle", "Car"]}));
        pretty_assertions::assert_eq!(kept.get("type"), Some(&json!(["Vehicle", "Car"])));
    }

    #[test]
    fn adapts_entity_array() {
        let value = json!([
            {
                "id": "urn:ngsi-ld:Entity:1",
                "type": "TestEntity",
                "expiresAt": "2025-01-01T00:00:00Z"
            },
            {
                "id": "urn:ngsi-ld:Entity:2",
                "type": "TestEntity",
                "scope": ["/A"]
            }
        ]);
        let adapted = adapt(SpecVersion::V1_3, value);
        let arr = adapted.as_array().expect("an adapted array");
        assert!(arr[0].get("expiresAt").is_none());
        assert!(arr[1].get("scope").is_none());
    }

    /// The entity's own members are reduced in every representation: Table
    /// 4.3.6.8-1 removes `expiresAt` below 1.9 and `scope` below 1.4, and note
    /// 1 collapses a multi-valued `type` below 1.3.
    #[test]
    fn the_entity_members_are_reduced_in_the_simplified_representation_too() {
        let value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": ["Vehicle", "Car"],
            "scope": ["/A"],
            "expiresAt": "2025-01-01T00:00:00Z",
            "speed": 10
        });
        let adapted = adapt_as(SpecVersion::V1_0, ResponseFormat::KeyValues, value);
        pretty_assertions::assert_eq!(adapted["type"], json!("Vehicle"));
        assert!(adapted.get("scope").is_none(), "{adapted}");
        assert!(adapted.get("expiresAt").is_none(), "{adapted}");
        pretty_assertions::assert_eq!(adapted["speed"], json!(10));
    }

    /// The format decides which reading an attribute gets, and the two readings
    /// answer differently for the same wrapper: clause 4.5.2.3's concise
    /// `LanguageProperty` is an instance whose value member moves, while clause
    /// 4.5.4's is the whole attribute and unwraps. Both land on the same JSON
    /// here, which is exactly why the format has to be stated rather than
    /// guessed -- the normalized reading of a clause 4.5.4 `VocabProperty`
    /// produces a `{"value": ...}` object that clause defines for nothing.
    #[test]
    fn each_format_routes_an_attribute_to_the_reading_that_representation_calls_for() {
        let entity = json!({"id": "urn:ngsi-ld:Entity:1", "type": "TestEntity", "category": {"vocab": "commercial"}});

        pretty_assertions::assert_eq!(
            adapt_as(SpecVersion::V1_4, ResponseFormat::KeyValues, entity.clone())["category"],
            json!("commercial")
        );
        pretty_assertions::assert_eq!(
            adapt_as(SpecVersion::V1_4, ResponseFormat::Simplified, entity.clone())["category"],
            json!("commercial")
        );
        pretty_assertions::assert_eq!(
            adapt_as(SpecVersion::V1_4, ResponseFormat::Concise, entity.clone())["category"],
            json!("commercial")
        );
        pretty_assertions::assert_eq!(
            adapt_as(SpecVersion::V1_4, ResponseFormat::Normalized, entity)["category"],
            json!({"value": "commercial"})
        );
    }
}
