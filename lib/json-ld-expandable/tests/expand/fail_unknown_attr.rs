use json_ld_expandable::JsonLdExpandable;

mod jsonld {
    pub trait JsonLdExpandable {
        fn to_expanded(&self) -> serde_json::Value;
    }
}

#[derive(JsonLdExpandable)]
#[jsonld(type = "https://example.com/Thing")]
pub struct UnknownAttr {
    #[jsonld(id)]
    pub id: String,

    #[jsonld(proprety = "https://example.com/name")]
    pub name: String,
}

fn main() {}
