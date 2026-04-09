use json_ld_expandable::JsonLdExpandable;

mod jsonld {
    pub trait JsonLdExpandable {
        fn to_expanded(&self) -> serde_json::Value;
    }
}

#[derive(JsonLdExpandable)]
#[jsonld(type = "https://example.com/TimeSeries")]
pub struct TimeSeries {
    #[jsonld(id)]
    pub id: String,

    #[jsonld(property = "https://example.com/timestamps", list, typed_value, datatype = "xsd:dateTime")]
    pub timestamps: Vec<String>,

    #[jsonld(property = "https://example.com/optionalStamps", list, typed_value, datatype = "ngsi:DateTime")]
    pub optional_stamps: Option<Vec<String>>,
}

fn main() {
    use jsonld::JsonLdExpandable;

    let series = TimeSeries {
        id: "urn:series:1".to_string(),
        timestamps: vec!["2024-01-01T00:00:00Z".to_string(), "2024-01-02T00:00:00Z".to_string()],
        optional_stamps: Some(vec!["2024-01-03T00:00:00Z".to_string()]),
    };

    let expanded = series.to_expanded();
    let obj = expanded.as_object().unwrap();

    // Verify list with typed values
    let ts = &obj["https://example.com/timestamps"];
    let ts_arr = ts.as_array().unwrap();
    assert_eq!(ts_arr.len(), 1);
    let list_obj = ts_arr[0].as_object().unwrap();
    let list_items = list_obj["@list"].as_array().unwrap();
    assert_eq!(list_items.len(), 2);
    assert_eq!(
        list_items[0],
        serde_json::json!({"@value": "2024-01-01T00:00:00Z", "@type": "http://www.w3.org/2001/XMLSchema#dateTime"})
    );
    assert_eq!(
        list_items[1],
        serde_json::json!({"@value": "2024-01-02T00:00:00Z", "@type": "http://www.w3.org/2001/XMLSchema#dateTime"})
    );

    // Verify optional list with ngsi: prefix expanded
    let opt = &obj["https://example.com/optionalStamps"];
    let opt_arr = opt.as_array().unwrap();
    let opt_list = opt_arr[0]["@list"].as_array().unwrap();
    assert_eq!(opt_list.len(), 1);
    assert_eq!(
        opt_list[0],
        serde_json::json!({"@value": "2024-01-03T00:00:00Z", "@type": "https://uri.etsi.org/ngsi-ld/DateTime"})
    );

    // Test with None
    let series_none = TimeSeries {
        id: "urn:series:2".to_string(),
        timestamps: vec![],
        optional_stamps: None,
    };
    let expanded = series_none.to_expanded();
    let obj = expanded.as_object().unwrap();
    assert!(obj.get("https://example.com/optionalStamps").is_none());
}
