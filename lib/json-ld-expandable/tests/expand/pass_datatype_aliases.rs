use json_ld_expandable::JsonLdExpandable;

mod jsonld {
    pub trait JsonLdExpandable {
        fn to_expanded(&self) -> serde_json::Value;
    }
}

#[derive(JsonLdExpandable)]
#[jsonld(type = "https://example.com/Record")]
pub struct Record {
    #[jsonld(id)]
    pub id: String,

    #[jsonld(property = "https://example.com/ngsiDate", typed_value, datatype = "ngsi:DateTime")]
    pub ngsi_date: String,

    #[jsonld(property = "https://example.com/xsdDate", typed_value, datatype = "xsd:dateTime")]
    pub xsd_date: String,

    #[jsonld(property = "https://example.com/xsdString", typed_value, datatype = "xsd:string")]
    pub xsd_string: String,

    #[jsonld(property = "https://example.com/xsdInt", typed_value, datatype = "xsd:integer")]
    pub xsd_int: i64,

    #[jsonld(property = "https://example.com/xsdDouble", typed_value, datatype = "xsd:double")]
    pub xsd_double: f64,

    #[jsonld(property = "https://example.com/xsdBool", typed_value, datatype = "xsd:boolean")]
    pub xsd_bool: bool,

    #[jsonld(property = "https://example.com/fullIri", typed_value, datatype = "https://custom.example/Type")]
    pub full_iri: String,
}

fn main() {
    use jsonld::JsonLdExpandable;

    let record = Record {
        id: "urn:record:1".to_string(),
        ngsi_date: "2024-01-01T00:00:00Z".to_string(),
        xsd_date: "2024-01-01T00:00:00Z".to_string(),
        xsd_string: "hello".to_string(),
        xsd_int: 42,
        xsd_double: 3.14,
        xsd_bool: true,
        full_iri: "custom".to_string(),
    };

    let expanded = record.to_expanded();
    let obj = expanded.as_object().unwrap();

    // ngsi: prefix
    assert_eq!(
        obj["https://example.com/ngsiDate"],
        serde_json::json!([{"@value": "2024-01-01T00:00:00Z", "@type": "https://uri.etsi.org/ngsi-ld/DateTime"}])
    );

    // xsd:dateTime
    assert_eq!(
        obj["https://example.com/xsdDate"],
        serde_json::json!([{"@value": "2024-01-01T00:00:00Z", "@type": "http://www.w3.org/2001/XMLSchema#dateTime"}])
    );

    // xsd:string
    assert_eq!(
        obj["https://example.com/xsdString"],
        serde_json::json!([{"@value": "hello", "@type": "http://www.w3.org/2001/XMLSchema#string"}])
    );

    // xsd:integer
    assert_eq!(
        obj["https://example.com/xsdInt"],
        serde_json::json!([{"@value": 42, "@type": "http://www.w3.org/2001/XMLSchema#integer"}])
    );

    // xsd:double
    assert_eq!(
        obj["https://example.com/xsdDouble"],
        serde_json::json!([{"@value": 3.14, "@type": "http://www.w3.org/2001/XMLSchema#double"}])
    );

    // xsd:boolean
    assert_eq!(
        obj["https://example.com/xsdBool"],
        serde_json::json!([{"@value": true, "@type": "http://www.w3.org/2001/XMLSchema#boolean"}])
    );

    // Full IRI (no prefix resolution)
    assert_eq!(
        obj["https://example.com/fullIri"],
        serde_json::json!([{"@value": "custom", "@type": "https://custom.example/Type"}])
    );
}
