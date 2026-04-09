use json_ld_expandable::JsonLdExpandable;

mod jsonld {
    pub trait JsonLdExpandable {
        fn to_expanded(&self) -> serde_json::Value;
    }
}

#[derive(JsonLdExpandable)]
#[jsonld(type = "https://uri.etsi.org/ngsi-ld/VocabProperty")]
pub struct VocabProp {
    #[jsonld(id)]
    pub id: String,

    #[jsonld(property = "https://uri.etsi.org/ngsi-ld/hasVocab", vocab_polymorphic)]
    pub vocab: serde_json::Value,
}

fn main() {
    use jsonld::JsonLdExpandable;

    // Test string vocab -> single @id
    let string_vocab = VocabProp {
        id: "urn:vocab:1".to_string(),
        vocab: serde_json::json!("categoryA"),
    };
    let expanded = string_vocab.to_expanded();
    let obj = expanded.as_object().unwrap();
    assert_eq!(
        obj["https://uri.etsi.org/ngsi-ld/hasVocab"],
        serde_json::json!([{"@id": "categoryA"}])
    );

    // Test array vocab -> multiple @id
    let array_vocab = VocabProp {
        id: "urn:vocab:2".to_string(),
        vocab: serde_json::json!(["catA", "catB"]),
    };
    let expanded = array_vocab.to_expanded();
    let obj = expanded.as_object().unwrap();
    assert_eq!(
        obj["https://uri.etsi.org/ngsi-ld/hasVocab"],
        serde_json::json!([{"@id": "catA"}, {"@id": "catB"}])
    );

    // Test null vocab -> property skipped
    let null_vocab = VocabProp {
        id: "urn:vocab:3".to_string(),
        vocab: serde_json::Value::Null,
    };
    let expanded = null_vocab.to_expanded();
    let obj = expanded.as_object().unwrap();
    assert!(obj.get("https://uri.etsi.org/ngsi-ld/hasVocab").is_none());

    // Test other value -> @value fallback
    let number_vocab = VocabProp {
        id: "urn:vocab:4".to_string(),
        vocab: serde_json::json!(42),
    };
    let expanded = number_vocab.to_expanded();
    let obj = expanded.as_object().unwrap();
    assert_eq!(
        obj["https://uri.etsi.org/ngsi-ld/hasVocab"],
        serde_json::json!([{"@value": 42}])
    );
}
