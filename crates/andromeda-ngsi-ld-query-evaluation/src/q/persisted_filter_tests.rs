use crate::{
    q::filter_run::filter_holds,
    queried_entity::{expanded_fixture::FixtureEntity, linked_entities::NoLinkedEntities},
};
use andromeda_ngsi_ld_query::{
    filter::{
        Filter,
        expansion::expand_filter,
        json_key_attributes::JsonKeyAttributes,
        language_matching::LanguageMatching,
        reading::filter_reading::FilterReading,
    },
    namespace_expander::EXAMPLE_NAMESPACE,
};
use serde_json::json;

/// The filter as a store reads it back: the expanded text, joined with the
/// written spelling of its members.
async fn persisted_round_trip(q: &str) -> Filter {
    let expanded = expand_filter(q.parse::<Filter>().expect("parses"), &JsonKeyAttributes::default(), &EXAMPLE_NAMESPACE)
        .await
        .expect("expands");
    let addressed: Filter = expanded.to_string().parse().expect("the expanded text parses");
    let written: Filter = expanded.written_member_text().parse().expect("the written text parses");
    addressed.with_written_members(&written)
}

/// Clause 4.9: a persisted `name[en]` still reads the English value of a
/// `LanguageProperty`, which the expanded text alone spells
/// `name["https://example.com/en"]`.
#[tokio::test]
async fn a_persisted_target_language_keeps_the_written_tag() {
    let door = FixtureEntity::from_expanded(&json!({
        "@id": "urn:ngsi-ld:Door:1",
        "@type": ["https://example.com/Door"],
        "https://example.com/name": [{
            "https://uri.etsi.org/ngsi-ld/hasLanguageMap": [
                {"@language": "en", "@value": "Door"},
                {"@language": "de", "@value": "Tuer"}
            ]
        }]
    }));
    let english = persisted_round_trip("name[en]==\"Door\"").await;
    assert!(filter_holds(
        &FilterReading::of(&english, LanguageMatching::TagEquality),
        &door,
        NoLinkedEntities
    ));
    let addressed_only: Filter = english.to_string().parse().expect("parses");
    assert!(
        !filter_holds(&FilterReading::of(&addressed_only, LanguageMatching::TagEquality), &door, NoLinkedEntities),
        "the expanded text alone has lost the tag"
    );
}
