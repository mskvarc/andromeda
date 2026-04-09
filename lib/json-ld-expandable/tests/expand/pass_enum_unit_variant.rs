use json_ld_expandable::JsonLdExpandable;

mod jsonld {
    pub trait JsonLdExpandable {
        fn to_expanded(&self) -> serde_json::Value;
    }
}

#[derive(JsonLdExpandable)]
pub enum Status {
    #[jsonld(type = "https://example.com/Active")]
    Active,

    #[jsonld(type = "https://example.com/Pending")]
    Pending,

    #[jsonld(type = "https://example.com/Completed")]
    Completed,
}

fn main() {
    use jsonld::JsonLdExpandable;

    let active = Status::Active;
    let expanded = active.to_expanded();
    let obj = expanded.as_object().unwrap();
    assert_eq!(obj["@type"], serde_json::json!(["https://example.com/Active"]));

    let pending = Status::Pending;
    let expanded = pending.to_expanded();
    let obj = expanded.as_object().unwrap();
    assert_eq!(obj["@type"], serde_json::json!(["https://example.com/Pending"]));

    let completed = Status::Completed;
    let expanded = completed.to_expanded();
    let obj = expanded.as_object().unwrap();
    assert_eq!(obj["@type"], serde_json::json!(["https://example.com/Completed"]));
}
