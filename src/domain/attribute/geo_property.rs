use crate::{
    domain::{attribute::AttributeValue, uri::Uri},
    jsonld::JsonLdExpandableMacro,
};
use chrono::{DateTime, Utc};
use geojson::Geometry;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

/// NGSI-LD GeoProperty representation (ETSI GS CIM 009 clause 5.2.7).
///
/// A GeoProperty represents an attribute whose value is a GeoJSON geometry.
/// Contains the geometry value, optional metadata (observedAt, datasetId),
/// and optional sub-attributes.
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, JsonLdExpandableMacro)]
#[serde(rename_all = "camelCase")]
#[jsonld(type = "ngsi:GeoProperty")]
pub struct GeoProperty {
    #[jsonld(property = "ngsi:hasValue", json_value)]
    pub value: Geometry,
    #[serde(default)]
    #[jsonld(property = "ngsi:observedAt", typed_value, datatype = "ngsi:DateTime")]
    pub observed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    #[jsonld(property = "ngsi:expiresAt", typed_value, datatype = "ngsi:DateTime")]
    pub expires_at: Option<DateTime<Utc>>,
    #[jsonld(property = "ngsi:datasetId", id_ref)]
    pub dataset_id: Option<Uri>,
    #[serde(flatten)]
    #[jsonld(flatten_map)]
    pub sub_attributes: IndexMap<String, AttributeValue>,
}

#[cfg(test)]
mod tests {
    use crate::jsonld::JsonLdExpandable;

    #[test]
    fn geo_property_expanded_has_json_typed_value() {
        let json = serde_json::json!({
            "type": "GeoProperty",
            "value": {"type": "Point", "coordinates": [13.0, 52.0]}
        });
        let attr: crate::domain::attribute::Attribute = serde_json::from_value(json).expect("geo_property");
        let crate::domain::attribute::Attribute::GeoProperty(g) = &attr else {
            panic!("expected GeoProperty");
        };
        let expanded = g.to_expanded();
        let obj = expanded.as_object().expect("expanded is object");

        assert_eq!(obj["@type"], serde_json::json!(["https://uri.etsi.org/ngsi-ld/GeoProperty"]));
        let has_value = &obj["https://uri.etsi.org/ngsi-ld/hasValue"];
        assert_eq!(has_value[0]["@type"], "@json");
        assert!(has_value[0]["@value"].is_object());
    }
}
