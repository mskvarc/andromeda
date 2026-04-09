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

    #[jsonld(vocab_polymorphic)]
    pub vocab: serde_json::Value,
}

fn main() {}
