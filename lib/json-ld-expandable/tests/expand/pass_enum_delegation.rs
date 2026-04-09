use json_ld_expandable::JsonLdExpandable;

mod jsonld {
    pub trait JsonLdExpandable {
        fn to_expanded(&self) -> serde_json::Value;
    }
}

#[derive(JsonLdExpandable)]
#[jsonld(type = "https://example.com/TypeA")]
pub struct TypeA {
    #[jsonld(id)]
    pub id: String,

    #[jsonld(property = "https://example.com/valA")]
    pub val: String,
}

#[derive(JsonLdExpandable)]
#[jsonld(type = "https://example.com/TypeB")]
pub struct TypeB {
    #[jsonld(id)]
    pub id: String,

    #[jsonld(property = "https://example.com/valB")]
    pub val: i64,
}

#[derive(JsonLdExpandable)]
pub enum MyEnum {
    A(TypeA),
    B(TypeB),
}

fn main() {
    use jsonld::JsonLdExpandable;

    let a = MyEnum::A(TypeA {
        id: "urn:a:1".to_string(),
        val: "hello".to_string(),
    });
    let expanded = a.to_expanded();
    let obj = expanded.as_object().unwrap();
    assert_eq!(obj["@type"], serde_json::json!(["https://example.com/TypeA"]));
    assert_eq!(obj["https://example.com/valA"], serde_json::json!([{"@value": "hello"}]));

    let b = MyEnum::B(TypeB {
        id: "urn:b:1".to_string(),
        val: 42,
    });
    let expanded = b.to_expanded();
    let obj = expanded.as_object().unwrap();
    assert_eq!(obj["@type"], serde_json::json!(["https://example.com/TypeB"]));
    assert_eq!(obj["https://example.com/valB"], serde_json::json!([{"@value": 42}]));
}
