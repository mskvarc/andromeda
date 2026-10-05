//! The clause 4.3.6.8 fallbacks over an attribute in the simplified temporal
//! representation of clause 4.5.9.
//!
//! That representation gives an attribute one object per `datasetId`, holding
//! the family's `type`, the `datasetId` when there is one, and a single plural
//! member -- `values`, `objects`, `languageMaps`, `jsons`, `vocabs`,
//! `valueLists` or `objectLists` -- whose value is the array of
//! `[value, Temporal Property]` pairs. The sequence of instances in time
//! therefore lives *inside* that member, which is what makes the outer array
//! the clause 4.5.5 set of instances separated by `datasetId` after all, and
//! leaves no `observedAt`, `unitCode` or `expiresAt` at attribute level for the
//! per-member rows of Tables 4.3.6.8-2 and -3 to reach.
//!
//! What is left of those tables is the family reformat, one level down from the
//! reified one: a family that postdates the target is rewritten as the family
//! it specializes, which means renaming the plural member to the one that
//! family carries and rewriting each pair's value the way clause 4.5.9 encodes
//! it there. The clause 4.5.19 aggregated representation is read here too: it
//! has the same shape, one object per `datasetId` naming its `type`, with the
//! aggregation vocabulary in place of the plural member -- which no row of the
//! fallback tables names, so it passes through while the family and the
//! `datasetId` are still adapted.

use crate::{
    object_list::object_list_member_target,
    version::fallback::{InstanceMember, VersionFallback},
};
use andromeda_ngsi_ld_lexicon::attribute_type::{AttributeType, TemporalValueEncoding};
use serde_json::{Map, Value};
use std::str::FromStr;

/// Adapt an attribute of a simplified temporal payload, which is either one
/// dataset's object or the array of them.
pub(crate) fn adapt_simplified_temporal_attribute(value: &mut Value, fallback: VersionFallback) {
    match value {
        Value::Array(datasets) if datasets.iter().any(Value::is_object) => {
            // Notes 2 and 3 of Table 4.3.6.8-1: instances separated by
            // `datasetId` exist from 1.3 onwards, and here the outer array is
            // exactly that set -- the time sequence is a member of each of its
            // objects, not the array itself.
            if fallback.collapses_instances() {
                let default_idx = datasets
                    .iter()
                    .position(|dataset| !dataset.as_object().is_some_and(|members| members.contains_key("datasetId")))
                    .unwrap_or(0);
                let mut default_dataset = if datasets.is_empty() {
                    Value::Null
                } else {
                    datasets.swap_remove(default_idx)
                };
                adapt_dataset(&mut default_dataset, fallback);
                *value = default_dataset;
                return;
            }
            for dataset in datasets.iter_mut() {
                adapt_dataset(dataset, fallback);
            }
        }
        Value::Object(_) => adapt_dataset(value, fallback),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_) => {}
    }
}

/// Adapt the object one `datasetId` group of a simplified temporal attribute is
/// rendered as.
fn adapt_dataset(dataset: &mut Value, fallback: VersionFallback) {
    let Some(obj) = dataset.as_object_mut() else {
        return;
    };
    // Clause 4.5.9 makes `type` mandatory on every one of these objects, so a
    // family is never inferred here the way clause 4.5.2.3 leaves it to be.
    let Some(family) = obj.get("type").and_then(Value::as_str).and_then(|raw| AttributeType::from_str(raw).ok()) else {
        return;
    };
    let generalization = fallback.family(family);
    if generalization != family {
        reformat_pairs(obj, family, generalization);
        obj.insert("type".to_string(), Value::String(generalization.as_ref().to_owned()));
    }
    // Table 4.3.6.8-2 and -3: `datasetId` is the only member of either table a
    // clause 4.5.9 attribute object can carry.
    if !fallback.keeps_instance_member(generalization, InstanceMember::DatasetId) {
        obj.remove("datasetId");
    }
}

/// Move the pairs of `family` to the member `generalization` carries, rewriting
/// each pair's value into the encoding clause 4.5.9 gives it there.
///
/// An aggregated payload carries no such member, and passes through untouched:
/// clause 4.5.19's aggregation vocabulary appears in no row of the fallback
/// tables, so there is nothing to move and nothing to rewrite.
fn reformat_pairs(obj: &mut Map<String, Value>, family: AttributeType, generalization: AttributeType) {
    let Some(Value::Array(pairs)) = obj.remove(family.temporal_values_key()) else {
        return;
    };
    let rewritten = pairs
        .into_iter()
        .map(|pair| match pair {
            Value::Array(mut elements) if !elements.is_empty() => {
                let observed = elements.pop().unwrap_or(Value::Null);
                let carried = elements.pop().unwrap_or(Value::Null);
                Value::Array(vec![rewrite_value(carried, family, generalization), observed])
            }
            other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_) | Value::Array(_)) => other,
        })
        .collect();
    obj.insert(generalization.temporal_values_key().to_string(), Value::Array(rewritten));
}

/// Rewrite the value half of one pair from the encoding `family` gives it to
/// the one `generalization` gives it.
///
/// `languageMap`, `json` and `vocab` are carried wrapped in a one-member object
/// (clause 4.5.9) and a Property's value bare, so the wrapper is unwrapped --
/// which is the same move the reified fallback makes when it renames the value
/// member. A `ListRelationship` carries an ordered array of targets where a
/// Relationship carries one `object`, so the first target stands for it, as
/// Table 4.3.6.8-1 has the reified collapse do.
fn rewrite_value(carried: Value, family: AttributeType, generalization: AttributeType) -> Value {
    if family == AttributeType::ListRelationship {
        return match &carried {
            Value::Array(targets) => targets
                .first()
                .and_then(object_list_member_target)
                .map_or(Value::Null, |target| Value::String(target.to_owned())),
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_) => carried,
        };
    }
    match (family.temporal_value_encoding(), generalization.temporal_value_encoding()) {
        (TemporalValueEncoding::Wrapped, TemporalValueEncoding::Bare) => match carried {
            Value::Object(mut members) => match members.remove(family.compact_value_key()) {
                Some(inner) => inner,
                None => Value::Object(members),
            },
            other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_)) => other,
        },
        (TemporalValueEncoding::Bare, TemporalValueEncoding::Bare | TemporalValueEncoding::Wrapped)
        | (TemporalValueEncoding::Wrapped, TemporalValueEncoding::Wrapped) => carried,
    }
}

#[cfg(test)]
mod tests {
    use crate::version::{adapted_payload::AdaptedPayload, fallback::VersionFallback, simplified_temporal_attribute::adapt_simplified_temporal_attribute};
    use andromeda_ngsi_ld_lexicon::spec_version::SpecVersion;
    use serde_json::{Value, json};

    /// Adapt one attribute of a clause 4.5.9 payload.
    fn adapt(target: SpecVersion, mut attribute: Value) -> Value {
        let fallback = VersionFallback::new(target, AdaptedPayload::SimplifiedTemporalEvolution).expect("a target below the conformant version");
        adapt_simplified_temporal_attribute(&mut attribute, fallback);
        attribute
    }

    /// Table 4.3.6.8-1: a `LanguageProperty` arrived at 1.4 and is reformatted
    /// as a Property below it, which one level down means the clause 4.5.9
    /// `languageMaps` member becomes `values` and each pair's wrapper is gone.
    #[test]
    fn a_language_property_below_1_4_becomes_a_property_carrying_bare_values() {
        let adapted = adapt(
            SpecVersion::V1_3,
            json!({
                "type": "LanguageProperty",
                "languageMaps": [
                    [{"languageMap": {"en": "yes", "fr": "oui"}}, "2026-01-01T00:00:00Z"],
                    [{"languageMap": {"en": "no", "fr": "non"}}, "2026-01-02T00:00:00Z"]
                ]
            }),
        );
        pretty_assertions::assert_eq!(adapted["type"], json!("Property"));
        assert!(adapted.get("languageMaps").is_none(), "{adapted}");
        pretty_assertions::assert_eq!(
            adapted["values"],
            json!([
                [{"en": "yes", "fr": "oui"}, "2026-01-01T00:00:00Z"],
                [{"en": "no", "fr": "non"}, "2026-01-02T00:00:00Z"]
            ])
        );
    }

    /// The 1.8 Property subclasses fall back the same way: `vocabs` and `jsons`
    /// are wrapped, `valueLists` is already bare and keeps its ordered array.
    #[test]
    fn the_1_8_property_subclasses_below_1_8_become_properties() {
        let vocab = adapt(
            SpecVersion::V1_4,
            json!({"type": "VocabProperty", "vocabs": [[{"vocab": "Male"}, "2026-01-01T00:00:00Z"]]}),
        );
        pretty_assertions::assert_eq!(vocab["type"], json!("Property"));
        pretty_assertions::assert_eq!(vocab["values"], json!([["Male", "2026-01-01T00:00:00Z"]]));

        let json_property = adapt(
            SpecVersion::V1_4,
            json!({"type": "JsonProperty", "jsons": [[{"json": {"id": "1"}}, "2026-01-01T00:00:00Z"]]}),
        );
        pretty_assertions::assert_eq!(json_property["type"], json!("Property"));
        pretty_assertions::assert_eq!(json_property["values"], json!([[{"id": "1"}, "2026-01-01T00:00:00Z"]]));

        let list_property = adapt(
            SpecVersion::V1_4,
            json!({"type": "ListProperty", "valueLists": [[[1, 2, 3], "2026-01-01T00:00:00Z"]]}),
        );
        pretty_assertions::assert_eq!(list_property["type"], json!("Property"));
        pretty_assertions::assert_eq!(list_property["values"], json!([[[1, 2, 3], "2026-01-01T00:00:00Z"]]));
    }

    /// Table 4.3.6.8-1 reformats a `ListRelationship` as a Relationship, which
    /// carries one `object`; the first target of the ordered array stands for
    /// it, as the reified collapse has it.
    #[test]
    fn a_list_relationship_below_1_8_keeps_the_first_target_of_each_pair() {
        let adapted = adapt(
            SpecVersion::V1_4,
            json!({
                "type": "ListRelationship",
                "objectLists": [[["urn:ngsi-ld:Vehicle:1", "urn:ngsi-ld:Vehicle:2"], "2026-01-01T00:00:00Z"]]
            }),
        );
        pretty_assertions::assert_eq!(adapted["type"], json!("Relationship"));
        assert!(adapted.get("objectLists").is_none(), "{adapted}");
        pretty_assertions::assert_eq!(adapted["objects"], json!([["urn:ngsi-ld:Vehicle:1", "2026-01-01T00:00:00Z"]]));
    }

    /// A family every version has known keeps its member and its type, and the
    /// sequence inside that member is never shortened: clause 4.5.9 puts the
    /// instances in time there, not in the attribute's own array.
    #[test]
    fn a_property_keeps_its_whole_sequence_at_every_target() {
        let adapted = adapt(
            SpecVersion::V1_0,
            json!({"type": "Property", "values": [[10, "2026-01-01T00:00:00Z"], [20, "2026-01-02T00:00:00Z"], [30, "2026-01-03T00:00:00Z"]]}),
        );
        pretty_assertions::assert_eq!(adapted["type"], json!("Property"));
        pretty_assertions::assert_eq!(adapted["values"].as_array().map(Vec::len), Some(3));
    }

    /// Table 4.3.6.8-2 and -3: `datasetId` arrived at 1.3, and it is the only
    /// member of either table one of these objects can carry.
    #[test]
    fn the_dataset_id_is_removed_below_1_3_and_kept_at_1_3() {
        let attribute = json!({"type": "Property", "datasetId": "urn:ngsi-ld:Dataset:1", "values": [[10, "2026-01-01T00:00:00Z"]]});
        assert!(adapt(SpecVersion::V1_0, attribute.clone()).get("datasetId").is_none());
        pretty_assertions::assert_eq!(adapt(SpecVersion::V1_3, attribute)["datasetId"], json!("urn:ngsi-ld:Dataset:1"));
    }

    /// Notes 2 and 3 of Table 4.3.6.8-1 do reach this representation: the outer
    /// array is one object per `datasetId`, which is the set they speak of.
    #[test]
    fn the_dataset_set_collapses_to_its_default_below_1_3_and_survives_at_1_3() {
        let attribute = json!([
            {"type": "Property", "values": [[10, "2026-01-01T00:00:00Z"]]},
            {"type": "Property", "datasetId": "urn:ngsi-ld:Dataset:1", "values": [[20, "2026-01-01T00:00:00Z"]]}
        ]);

        let collapsed = adapt(SpecVersion::V1_0, attribute.clone());
        assert!(collapsed.is_object(), "{collapsed}");
        pretty_assertions::assert_eq!(collapsed["values"], json!([[10, "2026-01-01T00:00:00Z"]]));

        let kept = adapt(SpecVersion::V1_3, attribute);
        pretty_assertions::assert_eq!(kept.as_array().map(Vec::len), Some(2));
    }

    /// The clause 4.5.19 aggregated representation shares the shape: its family
    /// and `datasetId` are adapted, and the aggregation vocabulary -- which no
    /// row of the fallback tables names -- passes through untouched.
    #[test]
    fn an_aggregated_attribute_keeps_its_aggregation_members() {
        let adapted = adapt(
            SpecVersion::V1_4,
            json!({
                "type": "VocabProperty",
                "datasetId": "urn:ngsi-ld:Dataset:1",
                "totalCount": [[3, "2026-01-01T00:00:00Z", "2026-01-02T00:00:00Z"]]
            }),
        );
        pretty_assertions::assert_eq!(adapted["type"], json!("Property"));
        pretty_assertions::assert_eq!(adapted["datasetId"], json!("urn:ngsi-ld:Dataset:1"));
        pretty_assertions::assert_eq!(adapted["totalCount"], json!([[3, "2026-01-01T00:00:00Z", "2026-01-02T00:00:00Z"]]));
    }

    /// An attribute naming no family this rendering understands is left alone
    /// rather than guessed at.
    #[test]
    fn an_object_naming_no_attribute_family_is_untouched() {
        let attribute = json!({"type": "NotAFamily", "values": [[1, "2026-01-01T00:00:00Z"]]});
        pretty_assertions::assert_eq!(adapt(SpecVersion::V1_0, attribute.clone()), attribute);
    }
}
