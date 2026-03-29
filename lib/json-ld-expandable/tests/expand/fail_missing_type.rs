use json_ld_expandable::JsonLdExpandable;

mod jsonld {
    pub trait JsonLdExpandable {
        fn to_expanded(&self) -> serde_json::Value;
    }
}

#[derive(JsonLdExpandable)]
pub struct MissingType {
    #[jsonld(id)]
    pub id: String,
}

fn main() {}
