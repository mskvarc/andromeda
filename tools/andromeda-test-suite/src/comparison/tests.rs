use crate::comparison::{ContextRule, matches_document};
use googletest::prelude::*;
use serde_json::json;

#[gtest]
fn an_object_with_the_expected_members_agrees_whatever_their_order() {
    expect_that!(
        &json!({"type": "Building", "id": "urn:a"}),
        matches_document(json!({"id": "urn:a", "type": "Building"}))
    );
}

#[gtest]
fn a_member_the_expectation_lacks_fails_unless_hidden() {
    let actual = json!({"speed": {"value": 1, "instanceId": "urn:i"}});
    expect_that!(&actual, not(matches_document(json!({"speed": {"value": 1}}))));
    expect_that!(&actual, matches_document(json!({"speed": {"value": 1}})).hiding("instanceId"));
    expect_that!(
        &json!({"speed": {"value": 1}}),
        matches_document(json!({"speed": {"value": 1}})).hiding("instanceId")
    );
}

#[gtest]
fn a_member_hidden_below_the_root_still_counts_at_the_root() {
    let expected = json!({"id": "urn:a", "speed": {"value": 1}});
    expect_that!(
        &json!({"id": "urn:a", "speed": {"value": 1, "instanceId": "urn:i"}}),
        matches_document(expected.clone()).hiding("instanceId")
    );
    expect_that!(
        &json!({"id": "urn:a", "instanceId": "urn:i", "speed": {"value": 1}}),
        not(matches_document(expected.clone()).hiding("instanceId"))
    );
    expect_that!(
        &json!({"id": "urn:a", "instanceId": "urn:i", "speed": {"value": 1}}),
        matches_document(expected).hiding_everywhere("instanceId")
    );
}

#[gtest]
fn a_missing_member_fails_and_names_its_path() {
    let differences = matches_document(json!({"speed": {"value": 1}})).differences(&json!({"speed": {}}));
    expect_that!(
        differences.iter().map(|difference| difference.path.as_str()).collect::<Vec<_>>(),
        elements_are![eq(&"$['speed']['value']")]
    );
}

#[gtest]
fn a_list_agrees_in_any_order_and_counts_its_items() {
    expect_that!(&json!([1, 2, 3]), matches_document(json!([3, 1, 2])));
    expect_that!(&json!([1, 1, 2]), not(matches_document(json!([1, 2]))));
    expect_that!(&json!([1, 1, 2]), not(matches_document(json!([1, 2, 2]))));
}

#[gtest]
fn entities_pair_by_id() {
    let actual = json!([{"id": "urn:b", "speed": 2}, {"id": "urn:a", "speed": 1}]);
    expect_that!(&actual, matches_document(json!([{"id": "urn:a", "speed": 1}, {"id": "urn:b", "speed": 2}])));
    expect_that!(
        &actual,
        not(matches_document(json!([{"id": "urn:a", "speed": 2}, {"id": "urn:b", "speed": 1}])))
    );
}

#[gtest]
fn objects_without_id_pair_by_content() {
    let actual = json!([{"value": 2, "observedAt": "2020-01-01T00:00:01Z"}, {"value": 1, "observedAt": "2020-01-01T00:00:00Z"}]);
    expect_that!(
        &actual,
        matches_document(json!([{"value": 1, "observedAt": "2020-01-01T00:00:00Z"}, {"value": 2, "observedAt": "2020-01-01T00:00:01Z"}]))
    );
}

#[gtest]
fn an_integer_differs_from_a_float_outside_a_list_and_not_below_one() {
    expect_that!(&json!({"value": 4.0}), not(matches_document(json!({"value": 4}))));
    expect_that!(&json!({"values": [4.0]}), matches_document(json!({"values": [4]})));
    expect_that!(&json!({"value": 4.5}), matches_document(json!({"value": 4.5})));
}

#[gtest]
fn observed_at_compares_as_an_instant_outside_a_list() {
    expect_that!(
        &json!({"observedAt": "2020-01-01T00:00:00.000Z"}),
        matches_document(json!({"observedAt": "2020-01-01T00:00:00Z"}))
    );
    expect_that!(
        &json!({"observedAt": "2020-01-01T00:00:01Z"}),
        not(matches_document(json!({"observedAt": "2020-01-01T00:00:00Z"})))
    );
}

#[gtest]
fn below_a_list_items_pair_by_text_first_and_the_few_left_by_instant() {
    let expected = json!([{"v": 1, "observedAt": "2020-01-01T00:00:00.000Z"}, {"v": 2, "observedAt": "2020-01-01T00:00:01.000Z"}]);
    let one_respelled = json!([{"v": 1, "observedAt": "2020-01-01T00:00:00Z"}, {"v": 2, "observedAt": "2020-01-01T00:00:01.000Z"}]);
    let both_respelled = json!([{"v": 1, "observedAt": "2020-01-01T00:00:00Z"}, {"v": 2, "observedAt": "2020-01-01T00:00:01Z"}]);
    expect_that!(&one_respelled, matches_document(expected.clone()));
    expect_that!(&both_respelled, not(matches_document(expected)));
    expect_that!(
        &json!([{"v": 1, "observedAt": "2020-01-01T00:00:00Z"}]),
        matches_document(json!([{"v": 1, "observedAt": "2020-01-01T00:00:00.000Z"}]))
    );
}

#[gtest]
fn an_item_left_to_the_diff_types_its_numbers() {
    let expected = json!([{"v": 4, "observedAt": "2020-01-01T00:00:00.000Z"}, {"v": 2, "observedAt": "2020-01-01T00:00:01.000Z"}]);
    expect_that!(
        &json!([{"v": 4.0, "observedAt": "2020-01-01T00:00:00.000Z"}, {"v": 2, "observedAt": "2020-01-01T00:00:01.000Z"}]),
        matches_document(expected.clone())
    );
    expect_that!(
        &json!([{"v": 4.0, "observedAt": "2020-01-01T00:00:00Z"}, {"v": 2, "observedAt": "2020-01-01T00:00:01.000Z"}]),
        not(matches_document(expected))
    );
}

#[gtest]
fn an_entity_grouped_by_id_is_diffed_with_typed_numbers() {
    expect_that!(&json!([{"id": "urn:a", "v": 4.0}]), not(matches_document(json!([{"id": "urn:a", "v": 4}]))));
    expect_that!(&json!([{"id": "urn:a", "v": 4}]), matches_document(json!([{"id": "urn:a", "v": 4}])));
}

#[gtest]
fn a_system_timestamp_is_any_well_formed_date() {
    let expected = json!({"createdAt": "2000-01-01T00:00:00Z"});
    expect_that!(&json!({"createdAt": "2026-10-24T21:00:00.123456Z"}), matches_document(expected.clone()));
    expect_that!(&json!({"createdAt": "yesterday"}), not(matches_document(expected.clone())));
    expect_that!(&json!({}), not(matches_document(expected)));
}

#[gtest]
fn a_context_string_equals_a_one_element_list() {
    expect_that!(&json!({"@context": ["https://c"]}), matches_document(json!({"@context": "https://c"})));
    expect_that!(&json!({"@context": "https://c"}), matches_document(json!({"@context": ["https://c"]})));
    expect_that!(
        &json!({"@context": ["https://c", "https://d"]}),
        not(matches_document(json!({"@context": "https://c"})))
    );
}

#[gtest]
fn any_core_context_version_stands_for_the_expected_one() {
    let expected = json!({"@context": "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld"});
    let rule = |expected| matches_document(expected).context_rule(ContextRule::AnyCoreContextVersion);
    expect_that!(
        &json!({"@context": ["https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld"]}),
        rule(expected.clone())
    );
    expect_that!(&json!({"@context": ["https://c"]}), not(rule(json!({"@context": "https://c"}))));
    expect_that!(&json!({"@context": "https://c"}), rule(json!({"@context": "https://c"})));
    expect_that!(&json!({"@context": "https://other"}), not(rule(expected)));
}

#[gtest]
fn a_list_beyond_the_cutoff_names_what_its_closest_items_differ_in() {
    let expected = json!([{"v": 1, "w": "a"}, {"v": 2, "w": "b"}]);
    let differences = matches_document(expected).differences(&json!([{"v": 1, "w": "x"}, {"v": 2, "w": "y"}]));
    expect_that!(
        differences.iter().map(|difference| difference.message.as_str()).collect::<Vec<_>>(),
        contains(starts_with("is \"x\""))
    );
    expect_that!(
        differences.iter().map(|difference| difference.message.as_str()).collect::<Vec<_>>(),
        contains(starts_with("is \"y\""))
    );
}
