use json_ld_expandable::JsonLdExpandable;

mod jsonld {
    pub trait JsonLdExpandable {
        fn to_expanded(&self) -> serde_json::Value;
    }
}

pub struct CustomField {
    pub inner: String,
}

impl jsonld::JsonLdExpandable for CustomField {
    fn to_expanded(&self) -> serde_json::Value {
        serde_json::json!([{"@value": self.inner}])
    }
}

#[derive(JsonLdExpandable)]
#[jsonld(type = "https://example.com/WithCustom")]
pub struct WithCustom {
    #[jsonld(id)]
    pub id: String,

    #[jsonld(property = "https://example.com/custom", custom)]
    pub field: CustomField,

    #[jsonld(property = "https://example.com/optCustom", custom)]
    pub opt_field: Option<CustomField>,
}

fn main() {
    use jsonld::JsonLdExpandable;

    let thing = WithCustom {
        id: "urn:custom:1".to_string(),
        field: CustomField { inner: "hello".to_string() },
        opt_field: Some(CustomField { inner: "world".to_string() }),
    };
    let expanded = thing.to_expanded();
    let obj = expanded.as_object().unwrap();

    assert_eq!(obj["https://example.com/custom"], serde_json::json!([{"@value": "hello"}]));
    assert_eq!(obj["https://example.com/optCustom"], serde_json::json!([{"@value": "world"}]));

    // None case
    let thing_none = WithCustom {
        id: "urn:custom:2".to_string(),
        field: CustomField { inner: "hi".to_string() },
        opt_field: None,
    };
    let expanded = thing_none.to_expanded();
    let obj = expanded.as_object().unwrap();
    assert!(obj.get("https://example.com/optCustom").is_none());
}
