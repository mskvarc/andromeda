use json_ld_expandable::JsonLdExpandable;

mod jsonld {
    pub trait JsonLdExpandable {
        fn to_expanded(&self) -> serde_json::Value;
    }
}

#[derive(JsonLdExpandable)]
#[jsonld(type = "https://example.com/Item")]
pub struct Item {
    #[jsonld(id)]
    pub id: String,

    #[jsonld(property = "https://example.com/name")]
    pub name: String,
}

#[derive(JsonLdExpandable)]
#[jsonld(type = "https://example.com/Container")]
pub struct Container {
    #[jsonld(id)]
    pub id: String,

    #[jsonld(property = "https://example.com/orderedItems", list, nested)]
    pub items: Vec<Item>,

    #[jsonld(property = "https://example.com/optionalItems", list, nested)]
    pub optional_items: Option<Vec<Item>>,
}

fn main() {
    use jsonld::JsonLdExpandable;

    let container = Container {
        id: "urn:container:1".to_string(),
        items: vec![
            Item {
                id: "urn:item:1".to_string(),
                name: "First".to_string(),
            },
            Item {
                id: "urn:item:2".to_string(),
                name: "Second".to_string(),
            },
        ],
        optional_items: Some(vec![Item {
            id: "urn:item:3".to_string(),
            name: "Optional".to_string(),
        }]),
    };

    let expanded = container.to_expanded();
    let obj = expanded.as_object().unwrap();

    // Verify list wrapping with nested expansion
    let items = &obj["https://example.com/orderedItems"];
    assert!(items.is_array());
    let items_arr = items.as_array().unwrap();
    assert_eq!(items_arr.len(), 1);
    let list_obj = items_arr[0].as_object().unwrap();
    let list_items = list_obj["@list"].as_array().unwrap();
    assert_eq!(list_items.len(), 2);
    assert_eq!(list_items[0]["@id"], "urn:item:1");
    assert_eq!(list_items[1]["@id"], "urn:item:2");

    // Verify optional list
    let opt_items = &obj["https://example.com/optionalItems"];
    assert!(opt_items.is_array());
    let opt_arr = opt_items.as_array().unwrap();
    let opt_list = opt_arr[0]["@list"].as_array().unwrap();
    assert_eq!(opt_list.len(), 1);
    assert_eq!(opt_list[0]["@id"], "urn:item:3");

    // Test with None
    let container_none = Container {
        id: "urn:container:2".to_string(),
        items: vec![],
        optional_items: None,
    };
    let expanded = container_none.to_expanded();
    let obj = expanded.as_object().unwrap();
    assert!(obj.get("https://example.com/optionalItems").is_none());
}
