//! One attribute instance of any NGSI-LD family, and the value side of an
//! entity's attribute map (ETSI GS CIM 009 clauses 5.2.5 and 4.5.5).

use crate::attribute::{
    common_fields::CommonAttributeFields,
    expansion::serde_json_value_to_json_value,
    geo_property::GeoProperty,
    json_property::JsonProperty,
    language_property::LanguageProperty,
    list_property::ListProperty,
    list_relationship::ListRelationship,
    property::Property,
    relationship::Relationship,
    vocab_property::VocabProperty,
};
use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
use chrono::{DateTime, Utc};
use jsonld::Expandable;
use jsonld_expandable_core::{JsonValue, ToJsonValue};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::iter::once;
use urn_rs::Urn;

/// One instance of any NGSI-LD attribute family, discriminated by its `type` member.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Attribute {
    /// A Property instance (clause 5.2.5).
    Property(Property),
    /// A Relationship instance (clause 5.2.6).
    Relationship(Relationship),
    /// A VocabProperty instance (clause 5.2.35).
    VocabProperty(VocabProperty),
    /// A GeoProperty instance (clause 5.2.7).
    GeoProperty(GeoProperty),
    /// A LanguageProperty instance (clause 5.2.32).
    LanguageProperty(LanguageProperty),
    /// A JsonProperty instance (clause 5.2.38).
    JsonProperty(JsonProperty),
    /// A ListProperty instance (clause 5.2.36).
    ListProperty(ListProperty),
    /// A ListRelationship instance (clause 5.2.37).
    ListRelationship(ListRelationship),
}

impl Attribute {
    /// Returns the family-specific payload member as raw JSON.
    ///
    /// Each family names its payload differently (`value`, `object`, `vocab`,
    /// …); this collapses them to the single JSON value a caller that does not
    /// care about the family needs.
    pub fn value(&self) -> Value {
        match self {
            Attribute::Property(p) => serde_json::to_value(&p.value).unwrap_or_default(),
            Attribute::Relationship(r) => serde_json::to_value(&r.object).unwrap_or_default(),
            Attribute::VocabProperty(v) => serde_json::to_value(&v.vocab).unwrap_or_default(),
            Attribute::GeoProperty(g) => serde_json::json!(g.value),
            // Serialization of IndexMap<LanguageMapKey, LanguageMapValue> to JSON cannot fail
            // (keys serialize to strings, values to strings or arrays of strings).
            Attribute::LanguageProperty(l) => serde_json::to_value(&l.language_map).unwrap_or_default(),
            // clone: the payload is already a `Value`, and the signature hands back an owned one.
            Attribute::JsonProperty(j) => j.json.clone(),
            Attribute::ListProperty(lp) => serde_json::to_value(&lp.value_list).unwrap_or_default(),
            Attribute::ListRelationship(lr) => serde_json::to_value(&lr.object_list).unwrap_or_default(),
        }
    }

    /// The attribute family this instance belongs to.
    ///
    /// The discriminant of the enum, exposed so a renderer can decide the shape
    /// a family takes (for example whether clause 4.5.4 keeps its value member)
    /// without re-parsing a `type` string.
    pub const fn attribute_type(&self) -> AttributeType {
        match self {
            Attribute::Property(_) => AttributeType::Property,
            Attribute::Relationship(_) => AttributeType::Relationship,
            Attribute::VocabProperty(_) => AttributeType::VocabProperty,
            Attribute::GeoProperty(_) => AttributeType::GeoProperty,
            Attribute::LanguageProperty(_) => AttributeType::LanguageProperty,
            Attribute::JsonProperty(_) => AttributeType::JsonProperty,
            Attribute::ListProperty(_) => AttributeType::ListProperty,
            Attribute::ListRelationship(_) => AttributeType::ListRelationship,
        }
    }

    /// The system members every attribute family carries.
    ///
    /// Every family struct embeds the same [`CommonAttributeFields`], so an
    /// accessor for a shared member reaches it through this one match rather
    /// than re-matching all eight families itself.
    pub const fn common(&self) -> &CommonAttributeFields {
        match self {
            Attribute::Property(p) => &p.common,
            Attribute::Relationship(r) => &r.common,
            Attribute::VocabProperty(v) => &v.common,
            Attribute::GeoProperty(g) => &g.common,
            Attribute::LanguageProperty(l) => &l.common,
            Attribute::JsonProperty(j) => &j.common,
            Attribute::ListProperty(lp) => &lp.common,
            Attribute::ListRelationship(lr) => &lr.common,
        }
    }

    /// Returns the `expiresAt` value of the underlying attribute instance, if set.
    pub const fn expires_at(&self) -> Option<DateTime<Utc>> {
        self.common().expires_at
    }

    /// Borrows the instance's `datasetId`, if set.
    pub const fn dataset_id(&self) -> Option<&Urn> {
        self.common().dataset_id.as_ref()
    }
}

/// The value side of one entry in an entity's attribute map.
///
/// A single attribute, a multi-attribute (clause 5.2.5), or a member the broker
/// carries through without interpreting.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AttributeValue {
    /// Exactly one attribute instance.
    Single(Box<Attribute>),
    /// Several instances of the same attribute, separated by `datasetId`.
    Multiple(Vec<Attribute>),
    /// A member that is not an attribute instance, preserved verbatim.
    Raw(Value),
}

impl Expandable for Attribute {
    fn expand<V: JsonValue>(&self) -> V {
        match self {
            Attribute::Property(p) => Expandable::expand::<V>(p),
            Attribute::Relationship(r) => Expandable::expand::<V>(r),
            Attribute::VocabProperty(v) => Expandable::expand::<V>(v),
            Attribute::GeoProperty(g) => Expandable::expand::<V>(g),
            Attribute::LanguageProperty(l) => Expandable::expand::<V>(l),
            Attribute::JsonProperty(j) => Expandable::expand::<V>(j),
            Attribute::ListProperty(lp) => Expandable::expand::<V>(lp),
            Attribute::ListRelationship(lr) => Expandable::expand::<V>(lr),
        }
    }
}

impl Expandable for AttributeValue {
    fn expand<V: JsonValue>(&self) -> V {
        match self {
            AttributeValue::Single(attr) => V::array(once(Expandable::expand::<V>(attr.as_ref()))),
            AttributeValue::Multiple(attrs) => V::array(attrs.iter().map(Expandable::expand::<V>)),
            AttributeValue::Raw(value) => V::array(once(V::object(once(("@value".to_string(), serde_json_value_to_json_value::<V>(value)))))),
        }
    }
}

impl<V: JsonValue> ToJsonValue<V> for AttributeValue {
    fn to_json_value(&self) -> V {
        Expandable::expand::<V>(self)
    }
}

#[cfg(test)]
mod tests {
    use crate::attribute::instance::*;
    use chrono::TimeZone;

    #[test]
    fn property_round_trip_with_expires_at() {
        let json = serde_json::json!({
            "type": "Property",
            "value": 25,
            "expiresAt": "2030-12-31T23:59:59Z"
        });
        let attr: Attribute = serde_json::from_value(json).unwrap();
        let Attribute::Property(ref p) = attr else {
            unreachable!("expected Property");
        };
        pretty_assertions::assert_eq!(p.common.expires_at, Some(Utc.with_ymd_and_hms(2030, 12, 31, 23, 59, 59).unwrap()));
        let reserialized = serde_json::to_value(&attr).unwrap();
        pretty_assertions::assert_eq!(reserialized["expiresAt"], "2030-12-31T23:59:59Z");
    }

    #[test]
    fn property_omits_expires_at_when_none() {
        let json = serde_json::json!({
            "type": "Property",
            "value": 42
        });
        let attr: Attribute = serde_json::from_value(json).unwrap();
        let serialized = serde_json::to_value(&attr).unwrap();
        assert!(serialized.get("expiresAt").is_none());
    }

    #[test]
    fn relationship_round_trip_with_expires_at() {
        let json = serde_json::json!({
            "type": "Relationship",
            "object": "urn:ngsi-ld:Entity:1",
            "expiresAt": "2030-06-15T12:00:00Z"
        });
        let attr: Attribute = serde_json::from_value(json).unwrap();
        let Attribute::Relationship(ref r) = attr else {
            unreachable!("expected Relationship");
        };
        pretty_assertions::assert_eq!(r.common.expires_at, Some(Utc.with_ymd_and_hms(2030, 6, 15, 12, 0, 0).unwrap()));
        let reserialized = serde_json::to_value(&attr).unwrap();
        pretty_assertions::assert_eq!(reserialized["expiresAt"], "2030-06-15T12:00:00Z");
    }

    #[test]
    fn geo_property_round_trip_with_expires_at() {
        let json = serde_json::json!({
            "type": "GeoProperty",
            "value": {"type": "Point", "coordinates": [13.0, 52.0]},
            "expiresAt": "2030-01-01T00:00:00Z"
        });
        let attr: Attribute = serde_json::from_value(json).unwrap();
        let Attribute::GeoProperty(ref g) = attr else {
            unreachable!("expected GeoProperty");
        };
        assert!(g.common.expires_at.is_some());
        let reserialized = serde_json::to_value(&attr).unwrap();
        pretty_assertions::assert_eq!(reserialized["expiresAt"], "2030-01-01T00:00:00Z");
    }

    #[test]
    fn vocab_property_round_trip_with_expires_at() {
        let json = serde_json::json!({
            "type": "VocabProperty",
            "vocab": "categoryA",
            "expiresAt": "2030-03-15T08:00:00Z"
        });
        let attr: Attribute = serde_json::from_value(json).unwrap();
        let Attribute::VocabProperty(ref v) = attr else {
            unreachable!("expected VocabProperty");
        };
        assert!(v.common.expires_at.is_some());
        let reserialized = serde_json::to_value(&attr).unwrap();
        pretty_assertions::assert_eq!(reserialized["expiresAt"], "2030-03-15T08:00:00Z");
    }

    #[test]
    fn language_property_round_trip_with_expires_at() {
        let json = serde_json::json!({
            "type": "LanguageProperty",
            "languageMap": {"en": "hello", "de": "hallo"},
            "expiresAt": "2030-07-01T00:00:00Z"
        });
        let attr: Attribute = serde_json::from_value(json).unwrap();
        let Attribute::LanguageProperty(ref l) = attr else {
            unreachable!("expected LanguageProperty");
        };
        assert!(l.common.expires_at.is_some());
        let reserialized = serde_json::to_value(&attr).unwrap();
        pretty_assertions::assert_eq!(reserialized["expiresAt"], "2030-07-01T00:00:00Z");
    }

    #[test]
    fn json_property_round_trip_with_expires_at() {
        let json = serde_json::json!({
            "type": "JsonProperty",
            "json": {"key": "value"},
            "expiresAt": "2030-09-01T12:00:00Z"
        });
        let attr: Attribute = serde_json::from_value(json).unwrap();
        let Attribute::JsonProperty(ref j) = attr else {
            unreachable!("expected JsonProperty");
        };
        assert!(j.common.expires_at.is_some());
        let reserialized = serde_json::to_value(&attr).unwrap();
        pretty_assertions::assert_eq!(reserialized["expiresAt"], "2030-09-01T12:00:00Z");
    }

    #[test]
    fn list_property_round_trip_with_expires_at() {
        let json = serde_json::json!({
            "type": "ListProperty",
            "valueList": [1, 2, 3],
            "expiresAt": "2030-11-20T06:30:00Z"
        });
        let attr: Attribute = serde_json::from_value(json).unwrap();
        let Attribute::ListProperty(ref lp) = attr else {
            unreachable!("expected ListProperty");
        };
        assert!(lp.common.expires_at.is_some());
        let reserialized = serde_json::to_value(&attr).unwrap();
        pretty_assertions::assert_eq!(reserialized["expiresAt"], "2030-11-20T06:30:00Z");
    }

    #[test]
    fn list_relationship_round_trip_with_expires_at() {
        let json = serde_json::json!({
            "type": "ListRelationship",
            "objectList": ["urn:ngsi-ld:Entity:1", "urn:ngsi-ld:Entity:2"],
            "expiresAt": "2030-04-10T18:00:00Z"
        });
        let attr: Attribute = serde_json::from_value(json).unwrap();
        let Attribute::ListRelationship(ref lr) = attr else {
            unreachable!("expected ListRelationship");
        };
        assert!(lr.common.expires_at.is_some());
        let reserialized = serde_json::to_value(&attr).unwrap();
        pretty_assertions::assert_eq!(reserialized["expiresAt"], "2030-04-10T18:00:00Z");
    }

    #[test]
    fn attribute_enum_expires_at_returns_value() {
        let dt = Utc.with_ymd_and_hms(2030, 6, 1, 0, 0, 0).unwrap();
        let json = serde_json::json!({
            "type": "Property",
            "value": 10,
            "expiresAt": "2030-06-01T00:00:00Z"
        });
        let attr: Attribute = serde_json::from_value(json).unwrap();
        pretty_assertions::assert_eq!(attr.expires_at(), Some(dt));
    }

    #[test]
    fn attribute_enum_expires_at_returns_none() {
        let json = serde_json::json!({
            "type": "Property",
            "value": 10
        });
        let attr: Attribute = serde_json::from_value(json).unwrap();
        pretty_assertions::assert_eq!(attr.expires_at(), None);
    }

    #[test]
    fn attribute_enum_delegates_to_expanded_to_inner_type() {
        let json = serde_json::json!({
            "type": "Property",
            "value": 42
        });
        let attr: Attribute = serde_json::from_value(json).unwrap();
        let expanded = attr.expand::<Value>();
        let obj = expanded.as_object().expect("expanded is object");

        // Enum delegation should produce the same output as the inner struct
        pretty_assertions::assert_eq!(obj["@type"], serde_json::json!(["https://uri.etsi.org/ngsi-ld/Property"]));
        pretty_assertions::assert_eq!(obj["https://uri.etsi.org/ngsi-ld/hasValue"], serde_json::json!([{"@value": 42}]));
    }

    #[test]
    fn attribute_enum_delegates_relationship_expansion() {
        let json = serde_json::json!({
            "type": "Relationship",
            "object": "urn:ngsi-ld:Entity:1"
        });
        let attr: Attribute = serde_json::from_value(json).unwrap();
        let expanded = attr.expand::<Value>();
        let obj = expanded.as_object().expect("expanded is object");

        pretty_assertions::assert_eq!(obj["@type"], serde_json::json!(["https://uri.etsi.org/ngsi-ld/Relationship"]));
    }

    #[test]
    fn attribute_value_single_wraps_in_array() {
        let json = serde_json::json!({
            "type": "Property",
            "value": 99
        });
        let attr: Attribute = serde_json::from_value(json).unwrap();
        let av = AttributeValue::Single(Box::new(attr));
        let expanded = av.expand::<Value>();
        let arr = expanded.as_array().expect("expanded is array");
        pretty_assertions::assert_eq!(arr.len(), 1);
        pretty_assertions::assert_eq!(arr[0]["@type"], serde_json::json!(["https://uri.etsi.org/ngsi-ld/Property"]));
    }

    #[test]
    fn attribute_value_multiple_produces_array() {
        let json1 = serde_json::json!({"type": "Property", "value": 1});
        let json2 = serde_json::json!({"type": "Property", "value": 2, "datasetId": "urn:ngsi-ld:Dataset:1"});
        let a1: Attribute = serde_json::from_value(json1).unwrap();
        let a2: Attribute = serde_json::from_value(json2).unwrap();
        let av = AttributeValue::Multiple(vec![a1, a2]);
        let expanded = av.expand::<Value>();
        let arr = expanded.as_array().expect("expanded is array");
        pretty_assertions::assert_eq!(arr.len(), 2);
    }

    /// One fully-populated instance per attribute family: the family's own
    /// members, `valueType`, the shared system members, and a sub-attribute.
    ///
    /// Folding the shared tail into [`CommonAttributeFields`] must leave the
    /// wire form untouched, so every one of these round-trips unchanged.
    fn fully_populated_instances() -> Vec<Value> {
        vec![
            serde_json::json!({
                "type": "Property",
                "value": 25,
                "valueType": "Integer",
                "previousValue": 24,
                "datasetId": "urn:ngsi-ld:Dataset:1",
                "observedAt": "2026-04-12T12:00:00Z",
                "expiresAt": "2030-12-31T23:59:59Z",
                "createdAt": "2024-01-01T00:00:00Z",
                "modifiedAt": "2024-01-02T00:00:00Z",
                "instanceId": "urn:ngsi-ld:attribute:instance:1",
                "accuracy": {"type": "Property", "value": 0.95}
            }),
            serde_json::json!({
                "type": "Relationship",
                "object": "urn:ngsi-ld:Entity:1",
                "objectType": "Building",
                "previousObject": "urn:ngsi-ld:Entity:0",
                "datasetId": "urn:ngsi-ld:Dataset:1",
                "observedAt": "2026-04-12T12:00:00Z",
                "providedBy": {"type": "Relationship", "object": "urn:ngsi-ld:Entity:9"}
            }),
            serde_json::json!({
                "type": "GeoProperty",
                "value": {"type": "Point", "coordinates": [13.0, 52.0]},
                "valueType": "geojson:Point",
                "previousValue": {"type": "Point", "coordinates": [12.0, 51.0]},
                "observedAt": "2026-04-12T12:00:00Z",
                "accuracy": {"type": "Property", "value": 1.5}
            }),
            serde_json::json!({
                "type": "VocabProperty",
                "vocab": "commercial",
                "valueType": "xsd:string",
                "previousVocab": "private",
                "datasetId": "urn:ngsi-ld:Dataset:1",
                "source": {"type": "Property", "value": "catalogue"}
            }),
            serde_json::json!({
                "type": "LanguageProperty",
                "languageMap": {"en": "hello", "de": "hallo"},
                "valueType": "langString",
                "previousLanguageMap": {"en": "hi"},
                "observedAt": "2026-04-12T12:00:00Z",
                "source": {"type": "Property", "value": "catalogue"}
            }),
            serde_json::json!({
                "type": "JsonProperty",
                "json": {"key": "value"},
                "valueType": "json",
                "previousJson": {"key": "old"},
                "createdAt": "2024-01-01T00:00:00Z",
                "source": {"type": "Property", "value": "catalogue"}
            }),
            serde_json::json!({
                "type": "ListProperty",
                "valueList": [1, 2, 3],
                "valueType": "Integer",
                "previousValueList": [0, 1, 2],
                "expiresAt": "2030-12-31T23:59:59Z",
                "source": {"type": "Property", "value": "catalogue"}
            }),
            serde_json::json!({
                "type": "ListRelationship",
                "objectList": ["urn:ngsi-ld:Entity:1", "urn:ngsi-ld:Entity:2"],
                "objectType": "Building",
                "previousObjectList": ["urn:ngsi-ld:Entity:0"],
                "modifiedAt": "2024-01-02T00:00:00Z",
                "providedBy": {"type": "Relationship", "object": "urn:ngsi-ld:Entity:9"}
            }),
        ]
    }

    #[test]
    fn a_unit_code_survives_a_string_round_trip_but_not_a_value_one() {
        // `cefact_units::UnitCode` deserializes only from a borrowed string, so
        // `serde_json::from_value` (used by the batch extractor and the CSR
        // forwarder) cannot supply one. Recorded here so the limitation is
        // visible rather than surfacing as a rejected entity.
        let json = r#"{"type":"Property","value":25,"unitCode":"CEL"}"#;
        let from_str: Attribute = serde_json::from_str(json).expect("parses from a string");
        pretty_assertions::assert_eq!(
            serde_json::to_value(&from_str).expect("serialises"),
            serde_json::from_str::<Value>(json).expect("json")
        );

        let value: Value = serde_json::from_str(json).expect("json");
        assert!(serde_json::from_value::<Attribute>(value).is_err());
    }

    #[test]
    fn every_attribute_family_reserialises_to_the_json_it_was_read_from() {
        for original in fully_populated_instances() {
            // clone: the original is compared against the round-tripped value below
            let attribute: Attribute = serde_json::from_value(original.clone()).expect("attribute parses");
            pretty_assertions::assert_eq!(serde_json::to_value(&attribute).expect("attribute serialises"), original);
        }
    }

    #[test]
    fn every_attribute_family_keeps_its_sub_attributes_out_of_its_own_members() {
        for original in fully_populated_instances() {
            let attribute: Attribute = serde_json::from_value(original).expect("attribute parses");
            let sub_attributes = match &attribute {
                Attribute::Property(p) => &p.common.sub_attributes,
                Attribute::Relationship(r) => &r.common.sub_attributes,
                Attribute::VocabProperty(v) => &v.common.sub_attributes,
                Attribute::GeoProperty(g) => &g.common.sub_attributes,
                Attribute::LanguageProperty(l) => &l.common.sub_attributes,
                Attribute::JsonProperty(j) => &j.common.sub_attributes,
                Attribute::ListProperty(lp) => &lp.common.sub_attributes,
                Attribute::ListRelationship(lr) => &lr.common.sub_attributes,
            };
            pretty_assertions::assert_eq!(sub_attributes.len(), 1, "{attribute:?}");
        }
    }

    #[test]
    fn common_reaches_the_shared_fields_of_every_family() {
        // `common()` must land on the same `CommonAttributeFields` the family
        // struct embeds, for every one of the eight families: each
        // fully-populated instance carries exactly one sub-attribute there, and
        // the `expires_at`/`dataset_id` accessors read through it.
        for original in fully_populated_instances() {
            let attribute: Attribute = serde_json::from_value(original).expect("attribute parses");
            let common = attribute.common();
            pretty_assertions::assert_eq!(common.sub_attributes.len(), 1, "{attribute:?}");
            pretty_assertions::assert_eq!(attribute.expires_at(), common.expires_at);
            pretty_assertions::assert_eq!(attribute.dataset_id(), common.dataset_id.as_ref());
        }
    }

    #[test]
    fn attribute_value_raw_wraps_in_value_object() {
        let av = AttributeValue::Raw(serde_json::json!("hello"));
        let expanded = av.expand::<Value>();
        pretty_assertions::assert_eq!(expanded, serde_json::json!([{"@value": "hello"}]));
    }
}
