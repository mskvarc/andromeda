use json_ld_expandable::JsonLdExpandable;

mod jsonld {
    pub trait JsonLdExpandable {
        fn to_expanded(&self) -> serde_json::Value;
    }
}

#[derive(JsonLdExpandable)]
#[jsonld(fragment)]
struct CommonFields {
    #[jsonld(property = "https://example.com/createdAt", typed_value, datatype = "xsd:dateTime")]
    created_at: String,

    #[jsonld(property = "https://example.com/modifiedAt", typed_value, datatype = "xsd:dateTime")]
    modified_at: String,
}

#[derive(JsonLdExpandable)]
#[jsonld(type = "https://example.com/Property")]
struct PropertyLike {
    #[jsonld(property = "https://example.com/value")]
    value: i32,

    #[jsonld(flatten_object)]
    common: CommonFields,
}

fn main() {
    use jsonld::JsonLdExpandable;

    let property = PropertyLike {
        value: 42,
        common: CommonFields {
            created_at: "2026-04-12T10:00:00Z".to_string(),
            modified_at: "2026-04-12T11:00:00Z".to_string(),
        },
    };

    let expanded = property.to_expanded();
    let obj = expanded.as_object().expect("object");

    assert_eq!(obj["@type"], serde_json::json!(["https://example.com/Property"]));
    assert!(obj.contains_key("https://example.com/value"));
    assert!(obj.contains_key("https://example.com/createdAt"));
    assert!(obj.contains_key("https://example.com/modifiedAt"));
}
