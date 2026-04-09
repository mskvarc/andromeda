use json_ld_expandable::JsonLdExpandable;

mod jsonld {
    pub trait JsonLdExpandable {
        fn to_expanded(&self) -> serde_json::Value;
    }
}

pub struct Inner;

#[derive(JsonLdExpandable)]
pub enum BadEnum {
    A(Inner),
    B,
}

fn main() {}
