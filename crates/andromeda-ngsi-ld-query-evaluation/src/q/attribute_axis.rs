//! Navigating the clause 4.9 attribute axis of an entity down to the target
//! values it addresses.
//!
//! Clause 4.9 gives an attribute reference three axes, and this module walks all
//! three: the `.`-separated sub-attribute chain, the trailing `[…]` group that
//! addresses a member of a compound value or the target language of a
//! `LanguageProperty`, and the `rel{…}` hop that crosses a Relationship into the
//! entity it names. What the axis terminates at, the target value each
//! attribute family carries, is
//! [`family_target`](crate::q::family_target)'s.
//!
//! An attribute may carry several instances (clause 4.5.5), and each of them
//! contributes its own target: clause 4.9 (p. 86) defines the comparison target
//! of a multi-instance attribute as "any Value of such instances".

use crate::{
    q::{
        axis_position::{AxisPosition, axis_positions},
        compound_member::{resolve_compound_member, walk_member_hops},
        language_target::language_target,
        target_value::{ResolvedTarget, json_member},
    },
    queried_entity::{
        attribute_reading::{QueriedEntity, QueriedInstance, QueriedValueNode},
        linked_entities::LinkedEntities,
        value_item::ValueItem,
    },
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType};
use andromeda_ngsi_ld_query::filter::{
    language_matching::LanguageMatching,
    path::{AttrPath, QueryPath},
    trailing_path::TrailingPath,
};

/// Resolve every value an attribute axis addresses, descending each
/// linked-entity hop into the entity it names in `linked`.
///
/// Clause 4.9 EXAMPLE 13: a hop "makes a sub-query on matching target objects",
/// the entities the Relationship's objects name. The entity under evaluation
/// names them and `linked` holds them; an object `linked` does not hold
/// reaches nothing. A target language selects languageMap keys by
/// `matching`.
#[must_use]
pub fn resolve_targets<'a, E: QueriedEntity<'a>, L: LinkedEntities<'a>>(
    entity: E,
    path: &QueryPath,
    linked: L,
    matching: LanguageMatching,
) -> Vec<ResolvedTarget<'a>> {
    match path {
        QueryPath::Local(local) => resolve_values(entity, local, matching),
        QueryPath::Linked {
            relationship,
            guard_types,
            inner,
        } => relationship_objects(entity, relationship)
            .filter_map(|object| linked.linked(object))
            .filter(|target| admitted_by_guard(*target, guard_types))
            .flat_map(|target| resolve_targets(target, inner, linked, matching))
            .collect(),
    }
}

/// The objects of every instance of the Relationship `relationship` an entity
/// holds: the target objects a hop through it crosses into.
///
/// Clause 4.9: the target object of a Relationship is "the object associated
/// (represented as a URI or array of URIs)", and with several instances "any
/// object of such instances". An attribute of another family names no linked
/// entity, whatever its value spells.
pub fn relationship_objects<'a, E: QueriedEntity<'a>>(entity: E, relationship: &AttributeName) -> impl Iterator<Item = &'a str> {
    entity
        .attribute_instances(relationship)
        .filter(|instance| instance.family() == AttributeType::Relationship)
        .flat_map(QueriedInstance::values)
        .filter_map(|item| match item {
            ValueItem::Reference(object) => Some(object),
            ValueItem::Node(node) => node.id(),
            ValueItem::Literal(_) | ValueItem::Json(_) | ValueItem::Opaque => None,
        })
}

/// Whether an attribute axis addresses an element the entity contains,
/// whatever value that element holds.
///
/// Clause 4.9: "When a Query Term only defines an attribute path (production
/// rule named `Attribute`), the matching Entities shall be those which define
/// the target element (Property or a Relationship), regardless of any target
/// value or object." Existence is therefore a question about the element, not
/// about a comparable value: a Property whose value is structured and a
/// `GeoProperty` held as a geometry carry no literal, yet each defines its
/// target element. Behind a trailing `[…]` path the element exists when the
/// member expression "result[s] in a defined value", here any value the member
/// holds.
#[must_use]
pub fn addresses_element<'a, E: QueriedEntity<'a>, L: LinkedEntities<'a>>(entity: E, path: &QueryPath, linked: L, matching: LanguageMatching) -> bool {
    match path {
        QueryPath::Local(local) => entity
            .attribute_instances(&local.attr)
            .any(|instance| instance_holds_element(instance, local, matching)),
        QueryPath::Linked {
            relationship,
            guard_types,
            inner,
        } => relationship_objects(entity, relationship)
            .filter_map(|object| linked.linked(object))
            .filter(|target| admitted_by_guard(*target, guard_types))
            .any(|target| addresses_element(target, inner, linked, matching)),
    }
}

/// Whether one attribute instance holds the element a path addresses.
///
/// With a target language the element is the key-value pair of that language
/// (clause 4.9), so it exists when a selected language holds a string.
fn instance_holds_element<'a, I: QueriedInstance<'a>>(instance: I, path: &AttrPath, matching: LanguageMatching) -> bool {
    axis_positions(instance, &path.sub_attrs)
        .into_iter()
        .any(|position| position_holds_element(position, path, matching))
}

/// Whether the element a path addresses is held at one position the
/// sub-attribute chain reaches.
fn position_holds_element<'a, I: QueriedInstance<'a>>(position: AxisPosition<'a, I>, path: &AttrPath, matching: LanguageMatching) -> bool {
    match &path.trailing {
        None | Some(TrailingPath::AllLanguages) => true,
        Some(TrailingPath::Language(_) | TrailingPath::Members(_)) => {
            if let Some((language_instance, language)) = position.target_language(path) {
                return language_instance.language_values(language, matching).next().is_some();
            }
            path.members().is_some_and(|members| {
                walk_member_hops(position.first_family_value(), &members.addressed).is_some()
                    || position.terminal().and_then(|target| json_member(&target, &members.written)).is_some()
            })
        }
    }
}

/// Whether a linked entity satisfies a hop's entity-type guard.
///
/// An empty guard admits every linked entity; otherwise clause 4.9's
/// `1*(EntityType)` list is a disjunction, so one shared type is enough.
fn admitted_by_guard<'a, E: QueriedEntity<'a>>(linked: E, guard_types: &[AttributeName]) -> bool {
    guard_types.is_empty()
        || linked
            .types()
            .any(|declared| guard_types.iter().any(|guard| guard.as_str() == declared.as_str()))
}

/// Resolve every target value an attribute path addresses in an entity.
///
/// Navigates the attribute -> each instance -> sub-attributes -> the family's
/// values, then the optional trailing group.
///
/// All instances are resolved, not just the first. Clause 4.9 (p. 86) defines
/// the comparison target for a multi-instance attribute as "any Value of such
/// instances", so a predicate holds when it holds for at least one of them,
/// whichever instance a `datasetId`-partitioned attribute writes first.
///
/// The `[*]` language-suppression marker selects no different resolution here.
/// Clause 4.9 defines a `LanguageProperty`'s target value with no target
/// language as "a value from any of the key-value pairs held within the
/// languageMap", which is the one array target the unqualified path already
/// addresses. The SQL compilation takes the same reading of the marker.
fn resolve_values<'a, E: QueriedEntity<'a>>(entity: E, path: &AttrPath, matching: LanguageMatching) -> Vec<ResolvedTarget<'a>> {
    entity
        .attribute_instances(&path.attr)
        .flat_map(|instance| axis_positions(instance, &path.sub_attrs))
        .filter_map(|position| resolve_position_value(position, path, matching))
        .collect()
}

/// Resolve one position of the axis down to the target value the predicate
/// compares.
///
/// A target language only a tag or range spells has no compound-member
/// reading, so it resolves nothing outside a `LanguageProperty`.
fn resolve_position_value<'a, I: QueriedInstance<'a>>(
    position: AxisPosition<'a, I>,
    path: &AttrPath,
    matching: LanguageMatching,
) -> Option<ResolvedTarget<'a>> {
    match &path.trailing {
        None | Some(TrailingPath::AllLanguages) => position.terminal(),
        Some(TrailingPath::Language(_) | TrailingPath::Members(_)) => {
            if let Some((language_instance, language)) = position.target_language(path) {
                return Some(language_target(language_instance, language, matching));
            }
            resolve_compound_member(position, path.members()?)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        q::attribute_axis::{addresses_element, resolve_targets},
        queried_entity::{
            expanded_fixture::{FixtureEntity, target_json},
            linked_entities::NoLinkedEntities,
        },
    };
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_query::filter::{
        language_matching::LanguageMatching,
        path::{AttrPath, QueryPath},
        reading::family_reading::TargetOrigin,
        trailing_path::{MemberPath, TrailingPath},
        value_path::parse_attribute_path,
    };
    use serde_json::{Value, json};

    /// An attribute name for a test path.
    fn name(raw: &str) -> AttributeName {
        raw.parse().expect("a valid attribute name")
    }

    /// The target values a path resolves to, with their origins.
    fn targets_along(entity: &Value, path: &AttrPath) -> Vec<(Value, TargetOrigin)> {
        targets_under(entity, path, LanguageMatching::TagEquality)
    }

    /// The target values a path resolves to under `matching`, with their
    /// origins.
    fn targets_under(entity: &Value, path: &AttrPath, matching: LanguageMatching) -> Vec<(Value, TargetOrigin)> {
        let entity = FixtureEntity::from_expanded(entity);
        resolve_targets(&entity, &QueryPath::Local(path.clone()), NoLinkedEntities, matching)
            .into_iter()
            .map(|target| (target_json(&target.value), target.origin))
            .collect()
    }

    /// A path whose trailing group is the one member `member`, as the client
    /// spelled it.
    fn bracketed(attribute: &str, member: &str) -> AttrPath {
        let mut path = AttrPath::simple(name(attribute));
        path.trailing = Some(TrailingPath::Members(MemberPath::new(vec![name(member)])));
        path
    }

    /// Clause 4.9: with a target language the target value is "the Value
    /// associated to the matching key-value pair held within the languageMap",
    /// every string of that language when it holds several, and nothing for a
    /// language the map does not hold.
    #[test]
    fn a_target_language_resolves_the_strings_of_that_language() {
        let entity = json!({
            "https://example.com/name": [{
                "https://uri.etsi.org/ngsi-ld/hasLanguageMap": [
                    {"@language": "en", "@value": "Door"},
                    {"@language": "de", "@value": "Tuer"}
                ]
            }],
            "https://example.com/tags": [{
                "https://uri.etsi.org/ngsi-ld/hasLanguageMap": [
                    {"@language": "en", "@value": "red"},
                    {"@language": "en", "@value": "cat"},
                    {"@language": "fr", "@value": "rouge"}
                ]
            }]
        });
        pretty_assertions::assert_eq!(
            targets_along(&entity, &bracketed("https://example.com/name", "en")),
            vec![(json!("Door"), TargetOrigin::AttributeValue)]
        );
        pretty_assertions::assert_eq!(
            targets_along(&entity, &bracketed("https://example.com/tags", "en")),
            vec![(json!(["red", "cat"]), TargetOrigin::AttributeValue)]
        );
        pretty_assertions::assert_eq!(
            targets_along(&entity, &bracketed("https://example.com/name", "fr")),
            vec![(json!([]), TargetOrigin::AttributeValue)]
        );
    }

    /// The target values one bare attribute path resolves to.
    fn targets_of(entity: &Value, attribute: &str) -> Vec<Value> {
        targets_along(entity, &AttrPath::simple(name(attribute)))
            .into_iter()
            .map(|(value, _)| value)
            .collect()
    }

    /// A path through the given sub-attribute segments.
    fn through(attribute: &str, sub_attrs: &[&str]) -> AttrPath {
        let mut path = AttrPath::simple(name(attribute));
        path.sub_attrs = sub_attrs.iter().map(|segment| name(segment)).collect();
        path
    }

    /// Clause 4.9 (p. 86): with several instances the target value "shall be any
    /// Value of such instances", so each instance contributes its own target.
    #[test]
    fn every_instance_of_a_multi_instance_attribute_contributes_a_target() {
        let entity = json!({
            "https://example.com/temp": [
                {"https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 20}]},
                {"https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 30}]}
            ]
        });
        pretty_assertions::assert_eq!(targets_of(&entity, "https://example.com/temp"), vec![json!(20), json!(30)]);
    }

    /// An attribute the entity does not carry addresses no target at all, which
    /// is what makes a bare existence Query Term fail on it (clause 4.9).
    #[test]
    fn an_absent_attribute_addresses_no_target() {
        let entity = json!({"https://example.com/temp": [{"https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 20}]}]});
        pretty_assertions::assert_eq!(targets_of(&entity, "https://example.com/other"), Vec::<Value>::new());
    }

    /// A system sub-attribute such as `observedAt` is held by the instance as
    /// data of its own; the path ends on its lexical form.
    #[test]
    fn an_instance_member_named_like_a_sub_attribute_is_its_own_target() {
        let entity = json!({
            "https://example.com/temp": [{
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 20}],
                "https://uri.etsi.org/ngsi-ld/observedAt": [{"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@value": "2024-06-01T00:00:00Z"}],
                "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": "urn:ngsi-ld:Dataset:a"}],
                "https://uri.etsi.org/ngsi-ld/unitCode": [{"@value": "CEL"}]
            }]
        });
        for (member, expected) in [
            ("https://uri.etsi.org/ngsi-ld/observedAt", "2024-06-01T00:00:00Z"),
            ("https://uri.etsi.org/ngsi-ld/datasetId", "urn:ngsi-ld:Dataset:a"),
            ("https://uri.etsi.org/ngsi-ld/unitCode", "CEL"),
        ] {
            pretty_assertions::assert_eq!(
                targets_along(&entity, &through("https://example.com/temp", &[member])),
                vec![(json!(expected), TargetOrigin::AttributeValue)],
                "{member}"
            );
        }
        pretty_assertions::assert_eq!(
            targets_along(&entity, &through("https://example.com/temp", &["https://uri.etsi.org/ngsi-ld/modifiedAt"])),
            Vec::new(),
            "an instance member the instance does not hold addresses nothing"
        );
        pretty_assertions::assert_eq!(
            targets_along(
                &entity,
                &through(
                    "https://example.com/temp",
                    &["https://uri.etsi.org/ngsi-ld/observedAt", "https://example.com/deeper"]
                )
            ),
            Vec::new(),
            "nothing is addressable beneath an instance member"
        );
    }

    /// Clause 4.9 makes the target value any Value of the instances when no
    /// datasetId is addressed, so a sub-attribute chain reads every instance
    /// of each hop.
    #[test]
    fn a_sub_attribute_chain_reads_every_instance_of_each_hop() {
        let entity = json!({
            "https://example.com/temp": [{
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 20}],
                "https://example.com/sensor": [
                    {
                        "https://uri.etsi.org/ngsi-ld/hasObject": [{"@id": "urn:ngsi-ld:Sensor:1"}],
                        "https://example.com/accuracy": [{"https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 0.5}]}]
                    },
                    {"https://uri.etsi.org/ngsi-ld/hasObject": [{"@id": "urn:ngsi-ld:Sensor:2"}]}
                ]
            }]
        });
        pretty_assertions::assert_eq!(
            targets_along(&entity, &through("https://example.com/temp", &["https://example.com/sensor"])),
            vec![
                (json!("urn:ngsi-ld:Sensor:1"), TargetOrigin::RelationshipObject),
                (json!("urn:ngsi-ld:Sensor:2"), TargetOrigin::RelationshipObject)
            ]
        );
        pretty_assertions::assert_eq!(
            targets_along(
                &entity,
                &through("https://example.com/temp", &["https://example.com/sensor", "https://example.com/accuracy"])
            ),
            vec![(json!(0.5), TargetOrigin::AttributeValue)]
        );
    }

    /// A segment naming the family's value member itself (`speed.value` once
    /// expanded) lands on the first value, read as a plain value even for a
    /// Relationship.
    #[test]
    fn a_segment_naming_the_value_member_lands_on_the_first_value() {
        let entity = json!({
            "https://example.com/speed": [{"https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 80}, {"@value": 90}]}],
            "https://example.com/owner": [{"https://uri.etsi.org/ngsi-ld/hasObject": [{"@id": "urn:ngsi-ld:Person:1"}]}],
            "https://example.com/colors": [{"https://uri.etsi.org/ngsi-ld/hasValueList": [{"@list": [{"@value": "red"}]}]}]
        });
        pretty_assertions::assert_eq!(
            targets_along(&entity, &through("https://example.com/speed", &["https://uri.etsi.org/ngsi-ld/hasValue"])),
            vec![(json!(80), TargetOrigin::AttributeValue)]
        );
        pretty_assertions::assert_eq!(
            targets_along(&entity, &through("https://example.com/owner", &["https://uri.etsi.org/ngsi-ld/hasObject"])),
            vec![(json!("urn:ngsi-ld:Person:1"), TargetOrigin::AttributeValue)]
        );
        pretty_assertions::assert_eq!(
            targets_along(&entity, &through("https://example.com/owner", &["https://uri.etsi.org/ngsi-ld/hasValue"])),
            Vec::new(),
            "another family's value member is no member of this instance"
        );
        pretty_assertions::assert_eq!(
            targets_along(&entity, &through("https://example.com/colors", &["https://uri.etsi.org/ngsi-ld/hasValueList"])),
            Vec::new(),
            "a list holds its items inside one list, which is no value of its own"
        );
    }

    /// A structured value's members are reachable through the `.` axis as well
    /// as through a compound path, one member per segment.
    #[test]
    fn a_structured_value_member_is_reached_through_the_value_member_segment() {
        let entity = json!({
            "https://example.com/address": [{
                "https://uri.etsi.org/ngsi-ld/hasValue": [{
                    "https://example.com/city": [{"@value": "Berlin"}]
                }]
            }]
        });
        pretty_assertions::assert_eq!(
            targets_along(
                &entity,
                &through(
                    "https://example.com/address",
                    &["https://uri.etsi.org/ngsi-ld/hasValue", "https://example.com/city"]
                )
            ),
            vec![(json!("Berlin"), TargetOrigin::AttributeValue)]
        );
    }

    /// The raw-JSON member of a compound path keeps the spelling the client
    /// wrote and stops at anything that is not a JSON object.
    #[test]
    fn a_raw_json_member_is_addressed_by_its_verbatim_key() {
        let entity = json!({
            "https://example.com/myJson": [{"https://uri.etsi.org/ngsi-ld/hasJSON": [{"@value": {"a": {"b": [1, 2]}}, "@type": "@json"}]}]
        });
        let addressed = vec![name("https://example.com/a"), name("https://example.com/b")];
        let mut path = AttrPath::simple(name("https://example.com/myJson"));
        path.trailing = Some(TrailingPath::Members(MemberPath {
            addressed: addressed.clone(),
            written: vec![name("a"), name("b")],
        }));
        pretty_assertions::assert_eq!(targets_along(&entity, &path), vec![(json!([1, 2]), TargetOrigin::AttributeValue)]);
        path.trailing = Some(TrailingPath::Members(MemberPath {
            addressed,
            written: vec![name("a"), name("b"), name("c")],
        }));
        pretty_assertions::assert_eq!(targets_along(&entity, &path), Vec::new());
    }

    /// A languageMap holding `en-GB`, `en`, `de-Latn-DE` and `fr`.
    fn multilingual() -> Value {
        json!({
            "https://example.com/name": [{
                "https://uri.etsi.org/ngsi-ld/hasLanguageMap": [
                    {"@language": "en-GB", "@value": "Lift"},
                    {"@language": "en", "@value": "Elevator"},
                    {"@language": "de-Latn-DE", "@value": "Aufzug"},
                    {"@language": "fr", "@value": "Ascenseur"}
                ]
            }]
        })
    }

    /// The strings `name[<language>]` reads under `matching`.
    fn read_language(language: &str, matching: LanguageMatching) -> Vec<Value> {
        let path = parse_attribute_path(&format!("\"https://example.com/name\"[{language}]")).expect("a target language");
        targets_under(&multilingual(), &path, matching).into_iter().map(|(value, _)| value).collect()
    }

    /// RFC 5646 section 2.1.1: case carries no distinction, under either mode.
    #[test]
    fn a_full_tag_reads_its_key_case_insensitively_under_both_modes() {
        for matching in [LanguageMatching::TagEquality, LanguageMatching::ExtendedFiltering] {
            pretty_assertions::assert_eq!(read_language("en-gb", matching), vec![json!("Lift")], "{matching:?}");
        }
    }

    /// Clause 4.9 matches the key to the target language; RFC 4647 section
    /// 3.3.2 extended filtering makes `en` a range over `en-GB` too.
    #[test]
    fn a_primary_tag_reads_its_subtags_only_under_extended_filtering() {
        pretty_assertions::assert_eq!(read_language("en", LanguageMatching::TagEquality), vec![json!("Elevator")]);
        pretty_assertions::assert_eq!(read_language("en", LanguageMatching::ExtendedFiltering), vec![json!(["Lift", "Elevator"])]);
    }

    #[test]
    fn a_wildcard_range_reads_every_key_it_matches() {
        for matching in [LanguageMatching::TagEquality, LanguageMatching::ExtendedFiltering] {
            pretty_assertions::assert_eq!(read_language("de-*-DE", matching), vec![json!("Aufzug")], "{matching:?}");
            pretty_assertions::assert_eq!(read_language("*-GB", matching), vec![json!("Lift")], "{matching:?}");
        }
    }

    /// The element of a target language exists when a selected key holds a
    /// string.
    #[test]
    fn a_target_language_element_exists_by_the_mode_selecting_it() {
        let entity = FixtureEntity::from_expanded(&json!({
            "https://example.com/name": [{
                "https://uri.etsi.org/ngsi-ld/hasLanguageMap": [{"@language": "en-GB", "@value": "Lift"}]
            }]
        }));
        let english = QueryPath::Local(parse_attribute_path("\"https://example.com/name\"[en]").expect("a path"));
        assert!(!addresses_element(&entity, &english, NoLinkedEntities, LanguageMatching::TagEquality));
        assert!(addresses_element(&entity, &english, NoLinkedEntities, LanguageMatching::ExtendedFiltering));
    }

    /// A target language no member name spells reads nothing outside a
    /// `LanguageProperty`.
    #[test]
    fn a_hyphenated_tag_addresses_no_compound_member() {
        let entity = json!({
            "https://example.com/address": [{
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"https://example.com/en-GB": [{"@value": "x"}]}]
            }]
        });
        let path = parse_attribute_path("\"https://example.com/address\"[en-GB]").expect("a path");
        pretty_assertions::assert_eq!(targets_along(&entity, &path), Vec::new());
    }
}
