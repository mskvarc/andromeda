use json_ld_expandable::JsonLdExpandable;

mod jsonld {
    pub trait JsonLdExpandable {
        fn to_expanded(&self) -> serde_json::Value;
    }
}

#[derive(JsonLdExpandable)]
#[jsonld(type = "https://example.com/Thing")]
pub struct Thing {
    #[jsonld(id)]
    pub id: String,

    #[jsonld(property = "https://example.com/count")]
    pub count: Option<i64>,

    #[jsonld(property = "https://example.com/names", vocab_vec)]
    pub names: Option<Vec<String>>,

    #[jsonld(property = "https://example.com/category", vocab)]
    pub category: Option<String>,

    #[jsonld(property = "https://example.com/tags", vocab_vec)]
    pub tags: Vec<String>,
}

fn main() {
    use jsonld::JsonLdExpandable;

    // Test with Some values
    let thing = Thing {
        id: "urn:thing:1".to_string(),
        count: Some(42),
        names: Some(vec!["alpha".to_string(), "beta".to_string()]),
        category: Some("https://example.com/Cat".to_string()),
        tags: vec!["https://example.com/A".to_string()],
    };
    let expanded = thing.to_expanded();
    let obj = expanded.as_object().unwrap();

    // count should be present
    let count = &obj["https://example.com/count"];
    assert_eq!(count, &serde_json::json!([{"@value": 42}]));

    // names: Option<Vec<String>> with vocab_vec should expand each item separately
    let names = &obj["https://example.com/names"];
    assert_eq!(names, &serde_json::json!([{"@id": "alpha"}, {"@id": "beta"}]));

    // category: Option<String> with vocab
    let category = &obj["https://example.com/category"];
    assert_eq!(category, &serde_json::json!([{"@id": "https://example.com/Cat"}]));

    // tags: Vec<String> with vocab_vec (non-optional)
    let tags = &obj["https://example.com/tags"];
    assert_eq!(tags, &serde_json::json!([{"@id": "https://example.com/A"}]));

    // Test with None values — fields should be absent
    let thing_none = Thing {
        id: "urn:thing:2".to_string(),
        count: None,
        names: None,
        category: None,
        tags: vec![],
    };
    let expanded = thing_none.to_expanded();
    let obj = expanded.as_object().unwrap();

    assert!(obj.get("https://example.com/count").is_none());
    assert!(obj.get("https://example.com/names").is_none());
    assert!(obj.get("https://example.com/category").is_none());
    // tags is not Option, so it's still present (empty array)
    assert!(obj.get("https://example.com/tags").is_some());
}
