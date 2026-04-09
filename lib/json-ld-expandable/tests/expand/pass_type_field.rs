use json_ld_expandable::JsonLdExpandable;

mod jsonld {
    pub trait JsonLdExpandable {
        fn to_expanded(&self) -> serde_json::Value;
    }

    pub trait JsonLdTypeValue {
        fn to_type_array(&self) -> serde_json::Value;
    }
}

pub enum DynamicType {
    Single(String),
    Multiple(Vec<String>),
}

impl jsonld::JsonLdTypeValue for DynamicType {
    fn to_type_array(&self) -> serde_json::Value {
        match self {
            DynamicType::Single(s) => serde_json::json!([s]),
            DynamicType::Multiple(v) => serde_json::json!(v),
        }
    }
}

#[derive(JsonLdExpandable)]
#[jsonld(type_field)]
pub struct DynamicEntity {
    #[jsonld(id)]
    pub id: String,

    #[jsonld(type_value)]
    pub type_: DynamicType,

    #[jsonld(property = "https://example.com/name")]
    pub name: String,
}

fn main() {
    use jsonld::JsonLdExpandable;

    let entity = DynamicEntity {
        id: "urn:entity:1".to_string(),
        type_: DynamicType::Single("https://example.com/Building".to_string()),
        name: "Test".to_string(),
    };
    let expanded = entity.to_expanded();
    let obj = expanded.as_object().unwrap();

    assert_eq!(obj["@type"], serde_json::json!(["https://example.com/Building"]));
    assert_eq!(obj["https://example.com/name"], serde_json::json!([{"@value": "Test"}]));

    let entity_multi = DynamicEntity {
        id: "urn:entity:2".to_string(),
        type_: DynamicType::Multiple(vec![
            "https://example.com/Building".to_string(),
            "https://example.com/Venue".to_string(),
        ]),
        name: "Multi".to_string(),
    };
    let expanded = entity_multi.to_expanded();
    let obj = expanded.as_object().unwrap();

    assert_eq!(
        obj["@type"],
        serde_json::json!(["https://example.com/Building", "https://example.com/Venue"])
    );
}
