use json_ld_expandable::JsonLdExpandable;

mod jsonld {
    pub trait JsonLdExpandable {
        fn to_expanded(&self) -> serde_json::Value;
    }
}

#[derive(JsonLdExpandable)]
#[jsonld(type = "https://example.com/JsonThing")]
pub struct JsonThing {
    #[jsonld(id)]
    pub id: String,

    #[jsonld(property = "https://example.com/data", json_value)]
    pub data: serde_json::Value,

    #[jsonld(property = "https://example.com/optData", json_value)]
    pub opt_data: Option<serde_json::Value>,
}

fn main() {
    use jsonld::JsonLdExpandable;

    let thing = JsonThing {
        id: "urn:thing:1".to_string(),
        data: serde_json::json!({"key": "value"}),
        opt_data: Some(serde_json::json!(42)),
    };
    let expanded = thing.to_expanded();
    let obj = expanded.as_object().unwrap();

    let data = &obj["https://example.com/data"];
    assert_eq!(data, &serde_json::json!([{"@value": {"key": "value"}, "@type": "@json"}]));

    let opt = &obj["https://example.com/optData"];
    assert_eq!(opt, &serde_json::json!([{"@value": 42, "@type": "@json"}]));

    // None case
    let thing_none = JsonThing {
        id: "urn:thing:2".to_string(),
        data: serde_json::json!(null),
        opt_data: None,
    };
    let expanded = thing_none.to_expanded();
    let obj = expanded.as_object().unwrap();
    assert!(obj.get("https://example.com/optData").is_none());
}
