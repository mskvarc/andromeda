use crate::{
    domain::attribute::{AttributeValue, common::CommonAttributeFields},
    jsonld::JsonLdExpandableMacro,
};
use geojson::Geometry;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

/// NGSI-LD GeoProperty representation (ETSI GS CIM 009 clause 5.2.7).
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, JsonLdExpandableMacro)]
#[serde(rename_all = "camelCase")]
#[jsonld(type = "ngsi:GeoProperty")]
pub struct GeoProperty {
    /// GeoJSON geometry value.
    #[jsonld(property = "ngsi:hasValue", json_value)]
    pub value: Geometry,
    /// Optional JSON-LD datatype for `value`.
    #[jsonld(property = "ngsi:hasValueType", vocab)]
    pub value_type: Option<compact_str::CompactString>,
    /// Previous geometry value emitted in notifications when `showChanges` is active.
    #[jsonld(property = "ngsi:hasPreviousValue", json_value)]
    pub previous_value: Option<Geometry>,
    #[serde(flatten)]
    #[jsonld(flatten_object)]
    pub common: CommonAttributeFields,
    #[serde(flatten)]
    #[jsonld(flatten_map)]
    pub sub_attributes: IndexMap<String, AttributeValue>,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use crate::{
        domain::attribute::Attribute,
        jsonld::{JsonLdExpandable, vocab},
    };

    #[test]
    fn geo_property_round_trip_supports_previous_value() {
        let json = serde_json::json!({
            "type": "GeoProperty",
            "value": { "type": "Point", "coordinates": [13.0, 52.0] },
            "previousValue": { "type": "Point", "coordinates": [12.0, 51.0] }
        });
        let attr: Attribute = serde_json::from_value(json).expect("geo property");
        let Attribute::GeoProperty(property) = attr else {
            unreachable!("expected GeoProperty");
        };
        assert!(property.previous_value.is_some());
    }

    #[test]
    fn geo_property_expanded_includes_value_type() {
        let json = serde_json::json!({
            "type": "GeoProperty",
            "value": { "type": "Point", "coordinates": [13.0, 52.0] },
            "valueType": "geojson:Point"
        });
        let attr: Attribute = serde_json::from_value(json).expect("geo property");
        let Attribute::GeoProperty(property) = attr else {
            unreachable!("expected GeoProperty");
        };
        let expanded = property.to_expanded();
        let obj = expanded.as_object().expect("object");
        assert!(obj.contains_key(vocab::expanded::VALUE_TYPE.as_str()));
    }
}
