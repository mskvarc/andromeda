use json_ld_expandable::JsonLdExpandable;

mod jsonld {
    pub trait JsonLdExpandable {
        fn to_expanded(&self) -> serde_json::Value;
    }
}

#[derive(JsonLdExpandable)]
#[jsonld(type = "https://uri.etsi.org/ngsi-ld/EntityTypeInfo")]
pub struct EntityTypeInfo {
    #[jsonld(id)]
    pub id: String,

    #[jsonld(property = "https://uri.etsi.org/ngsi-ld/typeName")]
    pub type_name: String,

    #[jsonld(property = "https://uri.etsi.org/ngsi-ld/entityCount")]
    pub entity_count: i64,
}

fn main() {
    let info = EntityTypeInfo {
        id: "urn:ngsi-ld:EntityTypeInfo:Vehicle".to_string(),
        type_name: "Vehicle".to_string(),
        entity_count: 42,
    };
    let expanded = jsonld::JsonLdExpandable::to_expanded(&info);
    assert!(expanded.is_object());
}
