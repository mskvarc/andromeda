use json_ld_expandable::JsonLdExpandable;

mod jsonld {
    pub trait JsonLdExpandable {
        fn to_expanded(&self) -> serde_json::Value;
    }
}

#[derive(JsonLdExpandable)]
#[jsonld(type = "https://example.com/ListRef")]
pub struct ListRef {
    #[jsonld(id)]
    pub id: String,

    #[jsonld(property = "https://example.com/objectList", list, id_ref)]
    pub object_list: Vec<String>,

    #[jsonld(property = "https://example.com/optList", list, id_ref)]
    pub opt_list: Option<Vec<String>>,
}

fn main() {
    use jsonld::JsonLdExpandable;

    let thing = ListRef {
        id: "urn:list:1".to_string(),
        object_list: vec!["urn:entity:1".to_string(), "urn:entity:2".to_string()],
        opt_list: Some(vec!["urn:entity:3".to_string()]),
    };
    let expanded = thing.to_expanded();
    let obj = expanded.as_object().unwrap();

    assert_eq!(
        obj["https://example.com/objectList"],
        serde_json::json!([{"@list": [{"@id": "urn:entity:1"}, {"@id": "urn:entity:2"}]}])
    );

    assert_eq!(
        obj["https://example.com/optList"],
        serde_json::json!([{"@list": [{"@id": "urn:entity:3"}]}])
    );

    // None case
    let thing_none = ListRef {
        id: "urn:list:2".to_string(),
        object_list: vec![],
        opt_list: None,
    };
    let expanded = thing_none.to_expanded();
    let obj = expanded.as_object().unwrap();
    assert!(obj.get("https://example.com/optList").is_none());
}
