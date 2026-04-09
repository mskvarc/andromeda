use json_ld_expandable::JsonLdExpandable;

mod jsonld {
    pub trait JsonLdExpandable {
        fn to_expanded(&self) -> serde_json::Value;
    }

    pub trait JsonLdTypeValue {
        fn to_type_array(&self) -> serde_json::Value;
    }
}

#[derive(JsonLdExpandable)]
#[jsonld(type = "https://example.com/Thing", type_field)]
pub struct Thing {
    #[jsonld(id)]
    pub id: String,

    #[jsonld(type_value)]
    pub type_: String,
}

fn main() {}
