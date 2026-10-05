//! The clause 4.3.6.8 fallbacks over an attribute rendered in the simplified
//! representation of clause 4.5.4.
//!
//! Clause 4.5.4 is lossy by construction: it keeps "only entity data (the
//! target object of each Relationship or the value of each Property)", so none
//! of the instance members Tables 4.3.6.8-2 and -3 remove is on the wire to be
//! removed. What is left of the fallback tables in this representation is Table
//! 4.3.6.8-1: the "Reformat attribute as Property" and "Reformat attribute as
//! Relationship" rows, and notes 2 and 3 on multiple instances.
//!
//! Family by family, with clause 4.5.4's own wording for each:
//!
//! - `Property` and `GeoProperty` (1.0) are "the Property Value" and no target
//!   predates them: unchanged.
//! - `Relationship` (1.0) is "the Relationship's Object (represented as a URI or
//!   array of URIs)": unchanged.
//! - `LanguageProperty` (1.4) is "a JSON Object containing a single Attribute
//!   with a key called `languageMap`". Reformatted as a Property below 1.4, its
//!   value is that `languageMap`, and a Property in this representation is its
//!   bare value: the wrapper is removed and nothing else. Which language the
//!   map holds is not the fallback's question -- clause 4.15's `lang` is what
//!   reduces a `languageMap` to one language, and inventing a selection here
//!   would answer a request the client did not make.
//! - `JsonProperty` (1.8) is "a JSON Object containing a single Attribute with a
//!   key called `json`", and `VocabProperty` (1.8) one "with a key called
//!   `vocab`": below 1.8 each unwraps to the bare value of the Property it is
//!   reformatted as.
//! - `ListProperty` (1.8) has no wrapper of its own in clause 4.5.4; a Property
//!   subtype's simplified rendering is "the Property Value", which for this
//!   family is the ordered array. Reformatting it as a Property leaves that same
//!   array, so the fallback is a no-op here rather than an amendment that cannot
//!   be made.
//! - `ListRelationship` (1.8) renders as the array of target URIs, and clause
//!   4.5.4 gives a plain Relationship exactly that shape -- "a URI or array of
//!   URIs", EXAMPLE 8. Reformatting it as a Relationship therefore also leaves
//!   the payload as it is. Reducing the array to its first element would not be
//!   the reformat: Table 4.3.6.8-3 types a 1.0 Relationship `object` as "String
//!   or String[]", so an array is what a 1.0 consumer expects, and a genuine
//!   multi-object Relationship is indistinguishable from a `ListRelationship`
//!   here, so the reduction would silently discard its targets.
//!
//! What remains is therefore the three wrappers of
//! [`SIMPLIFIED_WRAPPED_FAMILIES`] and the `dataset` map of clause 4.5.4's
//! multi-attribute case.

use crate::version::fallback::VersionFallback;
use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
use andromeda_ngsi_ld_query::dataset::DEFAULT_INSTANCE_TOKEN;
use andromeda_ngsi_ld_vocabulary::compact::properties as compact_properties;
use serde_json::Value;

/// The families clause 4.5.4 carries inside a one-member wrapper.
///
/// "For each `LanguageProperty`, a member whose key is the Property name (a term)
/// and whose value is a JSON Object containing a single Attribute with a key
/// called `languageMap`", and the same holds for a `JsonProperty`'s `json` and a
/// `VocabProperty`'s `vocab`. Every other family is its bare value, which
/// carries nothing Tables 4.3.6.8-2 and -3 name.
const SIMPLIFIED_WRAPPED_FAMILIES: [AttributeType; 3] = [AttributeType::LanguageProperty, AttributeType::VocabProperty, AttributeType::JsonProperty];

/// The sole member of a one-member JSON object, if the value is one.
fn sole_member(value: &Value) -> Option<&str> {
    let obj = value.as_object()?;
    if obj.len() != 1 {
        return None;
    }
    obj.keys().next().map(String::as_str)
}

/// Adapt one attribute of a simplified entity (clause 4.5.4).
///
/// Only two shapes carry anything the fallback tables act on, and both are
/// one-member JSON objects: the `dataset` map of the multi-attribute case, and
/// the `languageMap`, `json` and `vocab` wrappers of
/// [`SIMPLIFIED_WRAPPED_FAMILIES`]. Everything else is a bare value the module
/// documentation accounts for family by family, and is left exactly as it is.
///
/// The wrapper is all there is to key on, which is what makes this reading
/// lossy in one direction: a plain Property whose value happens to be a JSON
/// object with the single member `vocab`, `json` or `languageMap` is
/// indistinguishable from the family that reserves it, and a target below that
/// family's version unwraps it. Clause 4.5.4 keeps no `type` to tell the two
/// apart, so the ambiguity is in the representation rather than in this
/// reading of it.
pub(crate) fn adapt_simplified_attribute(value: &mut Value, fallback: VersionFallback) {
    let Some(member) = sole_member(value) else {
        return;
    };
    if member == compact_properties::DATASET {
        adapt_simplified_dataset_map(value, fallback);
        return;
    }
    let Some(family) = SIMPLIFIED_WRAPPED_FAMILIES.into_iter().find(|family| family.compact_value_key() == member) else {
        return;
    };
    if fallback.family(family) == family {
        return;
    }
    if let Some(inner) = value.as_object_mut().and_then(|obj| obj.remove(family.compact_value_key())) {
        *value = inner;
    }
}

/// Adapt the clause 4.5.4 `dataset` map an attribute with several instances is
/// carried in.
///
/// Table 4.3.6.8-1 notes 2 and 3: a target below 1.3 knew no `datasetId`, so it
/// receives one instance, "with preference to the default instance" -- the
/// `@none` key clause 4.5.4 gives it -- and receives it as the bare value a 1.0
/// simplified attribute is, not as a map. A 1.3 or newer target keeps the map,
/// with each instance adapted in its own right.
fn adapt_simplified_dataset_map(value: &mut Value, fallback: VersionFallback) {
    let Some(datasets) = value.as_object_mut().and_then(|obj| obj.get_mut(compact_properties::DATASET)) else {
        return;
    };
    let Some(datasets) = datasets.as_object_mut() else {
        return;
    };
    if !fallback.collapses_instances() {
        for instance in datasets.values_mut() {
            adapt_simplified_attribute(instance, fallback);
        }
        return;
    }
    let default_key = if datasets.contains_key(DEFAULT_INSTANCE_TOKEN) {
        Some(DEFAULT_INSTANCE_TOKEN.to_string())
    } else {
        datasets.keys().next().cloned() // clone: the key is read out before the map it indexes is drained
    };
    let Some(default_key) = default_key else {
        return;
    };
    let Some(mut default_instance) = datasets.remove(&default_key) else {
        return;
    };
    adapt_simplified_attribute(&mut default_instance, fallback);
    *value = default_instance;
}

#[cfg(test)]
mod tests {
    use crate::version::{adapted_payload::AdaptedPayload, fallback::VersionFallback, simplified_attribute::adapt_simplified_attribute};
    use andromeda_ngsi_ld_lexicon::spec_version::SpecVersion;
    use serde_json::{Value, json};

    /// Adapt one attribute of a clause 4.5.4 payload.
    fn adapt(target: SpecVersion, mut attribute: Value) -> Value {
        let fallback = VersionFallback::new(target, AdaptedPayload::Entity).expect("a target below the conformant version");
        adapt_simplified_attribute(&mut attribute, fallback);
        attribute
    }

    /// Clause 4.5.4 wraps a `LanguageProperty`, a `JsonProperty` and a
    /// `VocabProperty` in a one-member object; Table 4.3.6.8-1 reformats each as
    /// a Property, whose simplified rendering is the bare value.
    #[test]
    fn a_wrapped_family_unwraps_to_its_bare_value_below_the_version_that_introduced_it() {
        pretty_assertions::assert_eq!(adapt(SpecVersion::V1_3, json!({"languageMap": {"en": "Tower"}})), json!({"en": "Tower"}));
        pretty_assertions::assert_eq!(adapt(SpecVersion::V1_3, json!({"vocab": "commercial"})), json!("commercial"));
        pretty_assertions::assert_eq!(adapt(SpecVersion::V1_3, json!({"json": {"key": "value"}})), json!({"key": "value"}));
    }

    /// A target at or above the version a family was introduced at keeps the
    /// clause 4.5.4 wrapper: 1.4 defined the `LanguageProperty`, and 1.8 the
    /// `VocabProperty` and the `JsonProperty`.
    #[test]
    fn a_wrapper_survives_a_target_that_already_knew_the_family() {
        pretty_assertions::assert_eq!(
            adapt(SpecVersion::V1_4, json!({"languageMap": {"en": "Tower"}})),
            json!({"languageMap": {"en": "Tower"}})
        );
        pretty_assertions::assert_eq!(adapt(SpecVersion::V1_4, json!({"vocab": "commercial"})), json!("commercial"));
        pretty_assertions::assert_eq!(adapt(SpecVersion::V1_8, json!({"vocab": "commercial"})), json!({"vocab": "commercial"}));
        pretty_assertions::assert_eq!(adapt(SpecVersion::V1_8, json!({"json": {"key": "value"}})), json!({"json": {"key": "value"}}));
    }

    /// The families clause 4.5.4 renders bare carry nothing the fallback tables
    /// name, so every target receives them unchanged: a Property value, a
    /// `GeoProperty` geometry, a Relationship URI, a `ListProperty` array and a
    /// `ListRelationship` array of URIs.
    #[test]
    fn a_bare_family_is_unchanged_by_every_target() {
        for attribute in [
            json!(21),
            json!({"type": "Point", "coordinates": [1.0, 2.0]}),
            json!("urn:ngsi-ld:Building:1"),
            json!(["a", "b"]),
            json!(["urn:ngsi-ld:Room:1", "urn:ngsi-ld:Room:2"]),
        ] {
            pretty_assertions::assert_eq!(
                adapt(SpecVersion::V1_0, attribute.clone()),
                attribute,
                "a 1.0 target must not touch {attribute}"
            );
        }
    }

    /// Clause 4.5.4 carries several instances in a `dataset` map keyed by
    /// `datasetId`, with "the default datasetId (where present) ... represented
    /// by the JSON-LD keyword `@none`". Table 4.3.6.8-1 note 2 reduces that to
    /// the default instance alone for a target below 1.3, and leaves the map for
    /// a 1.3 target.
    #[test]
    fn a_dataset_map_collapses_to_its_default_instance_below_1_3_and_survives_at_1_3() {
        let dataset = json!({"dataset": {"urn:ds:1": 25.0, "@none": 30.0}});
        pretty_assertions::assert_eq!(adapt(SpecVersion::V1_0, dataset.clone()), json!(30.0));
        pretty_assertions::assert_eq!(adapt(SpecVersion::V1_3, dataset.clone()), dataset);
    }

    /// A `dataset` map with no `@none` entry has no default instance, so the
    /// first one stands in, and the wrapper it carries is unwrapped in turn.
    #[test]
    fn a_dataset_map_without_a_default_takes_its_first_instance_and_unwraps_it() {
        let adapted = adapt(SpecVersion::V1_0, json!({"dataset": {"urn:ds:1": {"languageMap": {"en": "Tower"}}}}));
        pretty_assertions::assert_eq!(adapted, json!({"en": "Tower"}));
    }

    /// A 1.3 target keeps the map and adapts each instance in its own right, so
    /// a wrapper a 1.3 target never knew is still unwrapped inside it.
    #[test]
    fn a_kept_dataset_map_has_each_instance_adapted_in_its_own_right() {
        let adapted = adapt(
            SpecVersion::V1_3,
            json!({"dataset": {"@none": {"vocab": "commercial"}, "urn:ds:1": {"vocab": "residential"}}}),
        );
        pretty_assertions::assert_eq!(adapted, json!({"dataset": {"@none": "commercial", "urn:ds:1": "residential"}}));
    }
}
