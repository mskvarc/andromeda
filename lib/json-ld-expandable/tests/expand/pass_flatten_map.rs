use json_ld_expandable::JsonLdExpandable;
use std::collections::HashMap;

mod jsonld {
    pub trait JsonLdExpandable {
        fn to_expanded(&self) -> serde_json::Value;
    }
}

#[derive(JsonLdExpandable)]
#[jsonld(type = "https://example.com/Inner")]
pub struct Inner {
    #[jsonld(id)]
    pub id: String,

    #[jsonld(property = "https://example.com/val")]
    pub val: String,
}

#[derive(JsonLdExpandable)]
#[jsonld(type = "https://example.com/Outer")]
pub struct Outer {
    #[jsonld(id)]
    pub id: String,

    #[jsonld(flatten_map)]
    pub attrs: HashMap<String, Inner>,
}

fn main() {
    use jsonld::JsonLdExpandable;

    let mut attrs = HashMap::new();
    attrs.insert(
        "https://example.com/child".to_string(),
        Inner {
            id: "urn:inner:1".to_string(),
            val: "hello".to_string(),
        },
    );

    let outer = Outer {
        id: "urn:outer:1".to_string(),
        attrs,
    };
    let expanded = outer.to_expanded();
    let obj = expanded.as_object().unwrap();

    assert!(obj.contains_key("https://example.com/child"));
}
