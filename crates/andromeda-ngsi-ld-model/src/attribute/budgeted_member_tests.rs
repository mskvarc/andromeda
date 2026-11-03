//! The members that grow without bound, read against the budget of values the
//! entity they belong to may still hold (clause 6.3.2): an entity within the
//! entity value limit is read whole, and one past it is cut one value past the
//! limit, which the count of the entity then refuses.

use crate::{
    attribute::{language_property::LanguageMap, object_list::RelationshipObjectList, one_or_many::OneOrMany},
    scope_member::ScopeMember,
};
use andromeda_ngsi_ld_entity::{document::slot::Slot, scopes::Scopes};
use andromeda_ngsi_ld_lexicon::{scope::path::ScopePath, uri::Uri};
use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
use andromeda_ngsi_ld_representation::value_budget::ValueBudget;
use serde::de::DeserializeSeed;
use std::num::NonZeroUsize;

fn budget(limit: usize) -> ValueBudget {
    ValueBudget::new(JsonValueLimit::new(NonZeroUsize::new(limit).expect("a positive limit")))
}

fn read<T, S: for<'de> DeserializeSeed<'de, Value = T>>(seed: S, text: &str) -> T {
    seed.deserialize(&mut serde_json::Deserializer::from_str(text)).expect("the member reads")
}

fn scopes(paths: &[&str]) -> Slot<Scopes> {
    Slot::Set(Scopes::try_from_paths(paths.iter().map(|path| path.parse::<ScopePath>().expect("a valid scope path"))).expect("at least one path"))
}

#[test]
fn a_language_map_of_exactly_the_limit_is_read_whole() {
    let budget = budget(3);
    let map = read(LanguageMap::within(Some(&budget)), r#"{"en":"a","fr":["b","c"]}"#);
    pretty_assertions::assert_eq!(map.string_count(), 3);
    assert!(!budget.is_spent());
}

#[test]
fn a_language_map_past_the_limit_is_cut_one_string_past_it() {
    let budget = budget(2);
    let map = read(LanguageMap::within(Some(&budget)), r#"{"aa":"1","bb":"2","cc":"3","dd":"4","ee":"5"}"#);
    pretty_assertions::assert_eq!(map.string_count(), 3);
    assert!(budget.is_spent());
}

#[test]
fn a_language_map_without_a_budget_is_read_whole() {
    let map = read(LanguageMap::within(None), r#"{"aa":"1","bb":"2","cc":"3","dd":"4","ee":"5"}"#);
    pretty_assertions::assert_eq!(map.string_count(), 5);
}

#[test]
fn a_tag_written_twice_counts_only_the_strings_it_ends_with() {
    let budget = budget(1);
    let map = read(LanguageMap::within(Some(&budget)), r#"{"en":"a","en":"b","en":"c"}"#);
    pretty_assertions::assert_eq!(map.string_count(), 1);
    assert!(!budget.is_spent());
}

#[test]
fn the_strings_of_a_cut_language_map_are_cut_inside_a_tag_as_well() {
    let budget = budget(2);
    let map = read(LanguageMap::within(Some(&budget)), r#"{"en":["a","b","c","d","e"]}"#);
    pretty_assertions::assert_eq!(map.string_count(), 3);
}

#[test]
fn scopes_of_exactly_the_limit_are_read_whole() {
    let budget = budget(2);
    let member = read(ScopeMember::within(Some(&budget)), r#"["/a","/b"]"#);
    pretty_assertions::assert_eq!(member.slot(), &scopes(&["/a", "/b"]));
    assert!(!budget.is_spent());
}

#[test]
fn scopes_past_the_limit_are_cut_one_scope_past_it() {
    let budget = budget(2);
    let member = read(ScopeMember::within(Some(&budget)), r#"["/a","/b","/c","/d","/e"]"#);
    pretty_assertions::assert_eq!(member.slot(), &scopes(&["/a", "/b", "/c"]));
}

#[test]
fn a_scope_written_again_is_one_scope_to_the_count() {
    let budget = budget(2);
    let member = read(ScopeMember::within(Some(&budget)), r#"["/a","/a","/a","/a","/b"]"#);
    pretty_assertions::assert_eq!(member.slot(), &scopes(&["/a", "/b"]));
    assert!(!budget.is_spent());
}

#[test]
fn object_targets_past_the_limit_are_cut_one_target_past_it() {
    let budget = budget(2);
    let targets = read(OneOrMany::<Uri>::within(Some(&budget)), r#"["urn:a:1","urn:a:2","urn:a:3","urn:a:4"]"#);
    pretty_assertions::assert_eq!(targets.iter().len(), 3);
}

#[test]
fn one_target_costs_one_value_of_the_budget() {
    let budget = budget(1);
    let targets = read(OneOrMany::<Uri>::within(Some(&budget)), r#""urn:a:1""#);
    pretty_assertions::assert_eq!(targets.iter().len(), 1);
    assert!(!budget.is_spent());
}

#[test]
fn object_list_entries_past_the_limit_are_cut_one_entry_past_it() {
    let budget = budget(2);
    let list = read(
        RelationshipObjectList::within(Some(&budget)),
        r#"["urn:a:1",{"object":"urn:a:2"},"urn:a:3","urn:a:4","urn:a:5"]"#,
    );
    pretty_assertions::assert_eq!(list.0.len(), 3);
}

#[test]
fn a_cut_member_skips_the_elements_it_does_not_record_without_reading_them() {
    let budget = budget(1);
    let list = read(
        RelationshipObjectList::within(Some(&budget)),
        r#"["urn:a:1","urn:a:2","not a uri",{"nonsense":[]}]"#,
    );
    pretty_assertions::assert_eq!(list.0.len(), 2);
}
