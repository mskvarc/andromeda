use json_ld_expandable::JsonLdExpandable;

mod jsonld {
    pub trait JsonLdExpandable {
        fn to_expanded(&self) -> serde_json::Value;
    }
}

#[derive(JsonLdExpandable)]
#[jsonld(type = "https://example.com/Ref")]
pub struct RefThing {
    #[jsonld(id)]
    pub id: String,

    #[jsonld(property = "https://example.com/target", id_ref)]
    pub target: String,

    #[jsonld(property = "https://example.com/optTarget", id_ref)]
    pub opt_target: Option<String>,
}

fn main() {
    use jsonld::JsonLdExpandable;

    let thing = RefThing {
        id: "urn:ref:1".to_string(),
        target: "urn:entity:42".to_string(),
        opt_target: Some("urn:entity:99".to_string()),
    };
    let expanded = thing.to_expanded();
    let obj = expanded.as_object().unwrap();

    assert_eq!(obj["https://example.com/target"], serde_json::json!([{"@id": "urn:entity:42"}]));
    assert_eq!(obj["https://example.com/optTarget"], serde_json::json!([{"@id": "urn:entity:99"}]));

    // None case
    let thing_none = RefThing {
        id: "urn:ref:2".to_string(),
        target: "urn:entity:1".to_string(),
        opt_target: None,
    };
    let expanded = thing_none.to_expanded();
    let obj = expanded.as_object().unwrap();
    assert!(obj.get("https://example.com/optTarget").is_none());
}
