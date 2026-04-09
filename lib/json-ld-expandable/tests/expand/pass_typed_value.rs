use json_ld_expandable::JsonLdExpandable;

mod jsonld {
    pub trait JsonLdExpandable {
        fn to_expanded(&self) -> serde_json::Value;
    }
}

#[derive(JsonLdExpandable)]
#[jsonld(type = "https://example.com/Sensor")]
pub struct Sensor {
    #[jsonld(id)]
    pub id: String,

    #[jsonld(property = "https://example.com/observedAt", typed_value, datatype = "https://uri.etsi.org/ngsi-ld/DateTime")]
    pub observed_at: Option<String>,

    #[jsonld(property = "https://example.com/createdAt", typed_value, datatype = "https://uri.etsi.org/ngsi-ld/DateTime")]
    pub created_at: String,
}

fn main() {
    use jsonld::JsonLdExpandable;

    let sensor = Sensor {
        id: "urn:sensor:1".to_string(),
        observed_at: Some("2024-01-01T00:00:00Z".to_string()),
        created_at: "2024-06-01T12:00:00Z".to_string(),
    };
    let expanded = sensor.to_expanded();
    let obj = expanded.as_object().unwrap();

    let observed = &obj["https://example.com/observedAt"];
    assert_eq!(observed, &serde_json::json!([{"@value": "2024-01-01T00:00:00Z", "@type": "https://uri.etsi.org/ngsi-ld/DateTime"}]));

    let created = &obj["https://example.com/createdAt"];
    assert_eq!(created, &serde_json::json!([{"@value": "2024-06-01T12:00:00Z", "@type": "https://uri.etsi.org/ngsi-ld/DateTime"}]));

    // Test with None
    let sensor_none = Sensor {
        id: "urn:sensor:2".to_string(),
        observed_at: None,
        created_at: "2024-06-01T12:00:00Z".to_string(),
    };
    let expanded = sensor_none.to_expanded();
    let obj = expanded.as_object().unwrap();
    assert!(obj.get("https://example.com/observedAt").is_none());
}
