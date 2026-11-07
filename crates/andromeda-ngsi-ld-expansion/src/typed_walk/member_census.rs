//! How many values the document census counts for the members whose expansion
//! does not map one to one onto them.
//!
//! The census counts one value per `object` target, one per `vocab` term, one
//! plus one per entry for a `languageMap` and an `objectList`, and one per
//! scope (the NGSI-LD Null that deletes the scope counts once). Counting the
//! items an expansion makes instead would refuse entities the limit admits: an
//! `objectList` entry expands to a node holding a node reference, two items,
//! while the census counts it once.

use andromeda_ngsi_ld_entity::document::slot::Slot;
use andromeda_ngsi_ld_model::{
    attribute::{language_property::LanguageMap, merge_value::LanguageMapPatch, object_list::RelationshipObjectList, one_or_many::OneOrMany},
    scope_member::ScopeMember,
};

/// The values of a Relationship `object` or a `VocabProperty` `vocab`: one per
/// element.
pub(crate) fn listed<T>(elements: &OneOrMany<T>) -> usize {
    elements.iter().len()
}

/// The values of a `ListRelationship` `objectList`: the list and each entry.
pub(crate) const fn object_list(list: &RelationshipObjectList) -> usize {
    list.0.len().saturating_add(1)
}

/// The values of a `LanguageProperty` `languageMap`: the map and each string.
pub(crate) fn language_map(map: &LanguageMap) -> usize {
    map.string_count().saturating_add(1)
}

/// The values of the `languageMap` a merge fragment writes: the map and each
/// string, the strings written untagged included.
pub(crate) fn language_map_patch(patch: &LanguageMapPatch) -> usize {
    match patch {
        LanguageMapPatch::Map(map) => language_map(map),
        LanguageMapPatch::Untagged(value) => value.string_count().saturating_add(1),
    }
}

/// The values of an entity `scope`: one per scope, one for its deletion.
pub(crate) fn scope(member: &ScopeMember) -> usize {
    match member.slot() {
        Slot::Set(scopes) => scopes.as_slice().len(),
        Slot::Remove => 1,
    }
}

#[cfg(test)]
mod tests {
    use crate::typed_walk::member_census::{language_map, language_map_patch, listed, object_list, scope};
    use andromeda_ngsi_ld_model::{
        attribute::{
            language_property::{LanguageMap, LanguageMapValue},
            merge_value::LanguageMapPatch,
            object_list::RelationshipObjectList,
            one_or_many::OneOrMany,
        },
        scope_member::ScopeMember,
    };
    use serde::de::DeserializeOwned;

    fn decoded<T: DeserializeOwned>(written: &str) -> T {
        serde_json::from_str(written).expect("the written member decodes")
    }

    #[test]
    fn one_element_or_several_count_one_per_element() {
        pretty_assertions::assert_eq!(listed(&decoded::<OneOrMany<String>>(r#""urn:a""#)), 1);
        pretty_assertions::assert_eq!(listed(&decoded::<OneOrMany<String>>(r#"["urn:a","urn:b","urn:c"]"#)), 3);
    }

    #[test]
    fn an_object_list_counts_itself_and_each_entry() {
        pretty_assertions::assert_eq!(object_list(&decoded::<RelationshipObjectList>(r#"["urn:ngsi-ld:T:1","urn:ngsi-ld:T:2"]"#)), 3);
        pretty_assertions::assert_eq!(object_list(&decoded::<RelationshipObjectList>("[]")), 1);
    }

    #[test]
    fn a_language_map_counts_itself_and_each_string_over_every_tag() {
        let map = decoded::<LanguageMap>(r#"{"en":"a","fr":["b","c"],"@none":"d"}"#);
        pretty_assertions::assert_eq!(language_map(&map), 5);
        pretty_assertions::assert_eq!(language_map_patch(&LanguageMapPatch::Map(map)), 5);
    }

    #[test]
    fn untagged_strings_of_a_merge_count_like_a_map_of_them() {
        let strings = LanguageMapValue::Multiple(vec!["x".to_owned(), "y".to_owned()]);
        pretty_assertions::assert_eq!(language_map_patch(&LanguageMapPatch::Untagged(strings)), 3);
    }

    #[test]
    fn a_scope_counts_one_per_scope_and_one_for_its_deletion() {
        pretty_assertions::assert_eq!(scope(&decoded::<ScopeMember>(r#"["/a","/b","/c"]"#)), 3);
        pretty_assertions::assert_eq!(scope(&decoded::<ScopeMember>(r#""/a""#)), 1);
        pretty_assertions::assert_eq!(scope(&decoded::<ScopeMember>(r#""urn:ngsi-ld:null""#)), 1);
    }
}
