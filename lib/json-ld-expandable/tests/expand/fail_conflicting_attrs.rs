use json_ld_expandable::JsonLdExpandable;

mod jsonld {
    pub trait JsonLdExpandable {
        fn to_expanded(&self) -> serde_json::Value;
    }
}

#[derive(JsonLdExpandable)]
#[jsonld(type = "https://example.com/Thing")]
pub struct Conflicting {
    #[jsonld(id)]
    pub id: String,

    #[jsonld(property = "https://example.com/value", nested, vocab)]
    pub value: String,
}

fn main() {}
