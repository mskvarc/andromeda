use json_ld_expandable::JsonLdExpandable;

mod jsonld {
    pub trait JsonLdExpandable {
        fn to_expanded(&self) -> serde_json::Value;
    }
}

#[derive(JsonLdExpandable)]
pub enum Event {
    #[jsonld(type = "https://example.com/Created")]
    Created {
        #[jsonld(id)]
        id: String,

        #[jsonld(property = "https://example.com/timestamp", typed_value, datatype = "xsd:dateTime")]
        timestamp: String,
    },

    #[jsonld(type = "https://example.com/Updated")]
    Updated {
        #[jsonld(id)]
        id: String,

        #[jsonld(property = "https://example.com/changes")]
        changes: i32,

        #[jsonld(property = "https://example.com/reason")]
        reason: Option<String>,
    },
}

fn main() {
    use jsonld::JsonLdExpandable;

    let created = Event::Created {
        id: "urn:event:1".to_string(),
        timestamp: "2024-01-01T00:00:00Z".to_string(),
    };
    let expanded = created.to_expanded();
    let obj = expanded.as_object().unwrap();
    assert_eq!(obj["@id"], "urn:event:1");
    assert_eq!(obj["@type"], serde_json::json!(["https://example.com/Created"]));
    assert_eq!(
        obj["https://example.com/timestamp"],
        serde_json::json!([{"@value": "2024-01-01T00:00:00Z", "@type": "http://www.w3.org/2001/XMLSchema#dateTime"}])
    );

    let updated = Event::Updated {
        id: "urn:event:2".to_string(),
        changes: 5,
        reason: Some("fix".to_string()),
    };
    let expanded = updated.to_expanded();
    let obj = expanded.as_object().unwrap();
    assert_eq!(obj["@id"], "urn:event:2");
    assert_eq!(obj["@type"], serde_json::json!(["https://example.com/Updated"]));
    assert_eq!(obj["https://example.com/changes"], serde_json::json!([{"@value": 5}]));
    assert_eq!(obj["https://example.com/reason"], serde_json::json!([{"@value": "fix"}]));

    // Test with None
    let updated_none = Event::Updated {
        id: "urn:event:3".to_string(),
        changes: 1,
        reason: None,
    };
    let expanded = updated_none.to_expanded();
    let obj = expanded.as_object().unwrap();
    assert!(obj.get("https://example.com/reason").is_none());
}
