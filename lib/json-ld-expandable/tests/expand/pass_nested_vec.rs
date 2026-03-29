use json_ld_expandable::JsonLdExpandable;

mod jsonld {
    pub trait JsonLdExpandable {
        fn to_expanded(&self) -> serde_json::Value;
    }
}

#[derive(JsonLdExpandable)]
#[jsonld(type = "https://example.com/Detail")]
pub struct Detail {
    #[jsonld(id)]
    pub id: String,

    #[jsonld(property = "https://example.com/value")]
    pub value: String,
}

#[derive(JsonLdExpandable)]
#[jsonld(type = "https://example.com/Parent")]
pub struct Parent {
    #[jsonld(id)]
    pub id: String,

    #[jsonld(property = "https://example.com/details", nested, vec)]
    pub details: Vec<Detail>,

    #[jsonld(property = "https://example.com/child", nested)]
    pub child: Detail,

    #[jsonld(property = "https://example.com/tags", list)]
    pub tags: Vec<String>,

    #[jsonld(property = "https://example.com/category", vocab)]
    pub category: String,

    #[jsonld(property = "https://example.com/types", vocab_vec)]
    pub types: Vec<String>,

    #[jsonld(skip)]
    pub internal: String,
}

fn main() {}
