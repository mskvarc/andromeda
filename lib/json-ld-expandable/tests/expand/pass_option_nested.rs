use json_ld_expandable::JsonLdExpandable;

mod jsonld {
    pub trait JsonLdExpandable {
        fn to_expanded(&self) -> serde_json::Value;
    }
}

#[derive(JsonLdExpandable)]
#[jsonld(type = "https://example.com/Geometry")]
pub struct Geometry {
    #[jsonld(id)]
    pub id: String,

    #[jsonld(property = "https://example.com/coordinates")]
    pub coordinates: String,
}

#[derive(JsonLdExpandable)]
#[jsonld(type = "https://example.com/Entity")]
pub struct Entity {
    #[jsonld(id)]
    pub id: String,

    #[jsonld(property = "https://example.com/location", nested)]
    pub location: Option<Geometry>,

    #[jsonld(property = "https://example.com/boundary", nested)]
    pub boundary: Option<Geometry>,
}

fn main() {
    use jsonld::JsonLdExpandable;

    // Test with Some value
    let entity_with_location = Entity {
        id: "urn:entity:1".to_string(),
        location: Some(Geometry {
            id: "urn:geo:1".to_string(),
            coordinates: "[0,0]".to_string(),
        }),
        boundary: None,
    };

    let expanded = entity_with_location.to_expanded();
    let obj = expanded.as_object().unwrap();

    // Verify location is present and expanded
    let loc = &obj["https://example.com/location"];
    let loc_arr = loc.as_array().unwrap();
    assert_eq!(loc_arr.len(), 1);
    assert_eq!(loc_arr[0]["@id"], "urn:geo:1");
    assert_eq!(loc_arr[0]["@type"], serde_json::json!(["https://example.com/Geometry"]));

    // Verify boundary is absent
    assert!(obj.get("https://example.com/boundary").is_none());

    // Test with all None
    let entity_no_geo = Entity {
        id: "urn:entity:2".to_string(),
        location: None,
        boundary: None,
    };

    let expanded = entity_no_geo.to_expanded();
    let obj = expanded.as_object().unwrap();
    assert!(obj.get("https://example.com/location").is_none());
    assert!(obj.get("https://example.com/boundary").is_none());
}
