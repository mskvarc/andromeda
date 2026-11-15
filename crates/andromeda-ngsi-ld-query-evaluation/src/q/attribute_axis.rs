//! Navigating the clause 4.9 attribute axis of an entity down to the target
//! values a term's reading addresses.
//!
//! Clause 4.9 gives an attribute reference three axes, and this module walks all
//! three: the `rel{…}` hop that crosses a Relationship into the entity it
//! names, the `.`-separated sub-attribute chain, and the terminal the reading
//! decided, the instance's own value, an instance member, a target language or
//! a member of a compound value. What the axis terminates at, the target
//! value each attribute family carries, is
//! [`family_target`](crate::q::family_target)'s.
//!
//! An attribute may carry several instances (clause 4.5.5), and each of them
//! contributes its own target: clause 4.9 (p. 86) defines the comparison target
//! of a multi-instance attribute as "any Value of such instances".

use crate::{
    q::{
        compound_member::{geometry_member_target, resolve_compound_member, walk_member_hops},
        family_target::{family_target, first_family_value},
        language_target::language_target,
        target_value::{ResolvedTarget, TargetValue, json_member},
    },
    queried_entity::{
        attribute_reading::{QueriedEntity, QueriedInstance, QueriedValueNode},
        linked_entities::LinkedEntities,
        value_item::ValueItem,
    },
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
use andromeda_ngsi_ld_query::filter::{
    reading::{
        family_reading::{MemberSource, MultiValueReading, TargetOrigin, ValueKeys, family_reading, hop_families},
        hop::Hop,
        term_reading::TermReading,
        terminal::Terminal,
    },
    target_language::LanguageSelector,
    trailing_path::MemberPath,
};

/// Whether some target a term's path addresses is accepted by `accepts`,
/// descending each linked-entity hop into the entity it names in `linked`.
///
/// Targets are visited in the order the axis reaches them (the objects of a
/// hop, the instances of the attribute, the instances of each sub-attribute,
/// then the terminal) and the walk stops at the first one `accepts` takes, so
/// no collection of targets is built.
///
/// Clause 4.9 EXAMPLE 13: a hop "makes a sub-query on matching target objects",
/// the entities the Relationship's objects name. The entity under evaluation
/// names them and `linked` holds them; an object `linked` does not hold
/// reaches nothing.
pub fn any_target<'a, E: QueriedEntity<'a>, L: LinkedEntities<'a>>(
    entity: E,
    term: &TermReading,
    linked: L,
    accepts: &mut impl FnMut(ResolvedTarget<'a>) -> bool,
) -> bool {
    target_beyond(entity, term.hops(), term, linked, accepts)
}

/// Whether `accepts` takes a target of `term` in `entity` once the `hops` left
/// are crossed.
fn target_beyond<'a, E: QueriedEntity<'a>, L: LinkedEntities<'a>>(
    entity: E,
    hops: &[Hop],
    term: &TermReading,
    linked: L,
    accepts: &mut impl FnMut(ResolvedTarget<'a>) -> bool,
) -> bool {
    match hops.split_first() {
        None => local_target(entity, term, accepts),
        Some((hop, further)) => hop_reaches(entity, hop, linked).any(|reached| target_beyond(reached, further, term, linked, accepts)),
    }
}

/// The objects of every instance of the Relationship or `ListRelationship`
/// `relationship` an entity holds: the target objects a hop through it crosses
/// into.
///
/// Clause 4.9: the target object of a Relationship is "the object associated
/// (represented as a URI or array of URIs)", that of a `ListRelationship` "the
/// array of objects", and with several instances "any object of such
/// instances". An attribute of a family no hop crosses ([`hop_families`])
/// names no linked entity, whatever its value spells.
pub fn relationship_objects<'a, E: QueriedEntity<'a>>(entity: E, relationship: &AttributeName) -> impl Iterator<Item = &'a str> {
    entity
        .attribute_instances(relationship)
        .filter(|instance| hop_families().any(|family| family == instance.family()))
        .flat_map(QueriedInstance::values)
        .filter_map(|item| match item {
            ValueItem::Reference(object) => Some(object),
            ValueItem::Node(node) => node.id(),
            ValueItem::Literal(_) | ValueItem::Json(_) | ValueItem::Geometry(_) | ValueItem::Opaque => None,
        })
}

/// The linked entities one hop reaches from `entity`: those `linked` holds
/// for the objects of its Relationship, of a type its guard admits.
fn hop_reaches<'a, E: QueriedEntity<'a>, L: LinkedEntities<'a>>(entity: E, hop: &Hop, linked: L) -> impl Iterator<Item = L::Entity> {
    relationship_objects(entity, hop.relationship())
        .filter_map(move |object| linked.linked(object))
        .filter(|reached| admitted_by_guard(*reached, hop.guard_types()))
}

/// Whether a term's path addresses an element the entity contains, whatever
/// value that element holds.
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
pub fn addresses_element<'a, E: QueriedEntity<'a>, L: LinkedEntities<'a>>(entity: E, term: &TermReading, linked: L) -> bool {
    element_beyond(entity, term.hops(), term, linked)
}

/// Whether `entity` holds the element of `term` once the `hops` left are
/// crossed.
fn element_beyond<'a, E: QueriedEntity<'a>, L: LinkedEntities<'a>>(entity: E, hops: &[Hop], term: &TermReading, linked: L) -> bool {
    match hops.split_first() {
        None => entity
            .attribute_instances(term.attribute())
            .any(|instance| any_instance_along(instance, term.sub_attributes(), &mut |reached| terminal_exists(reached, term.terminal()))),
        Some((hop, further)) => hop_reaches(entity, hop, linked).any(|reached| element_beyond(reached, further, term, linked)),
    }
}

/// Whether a linked entity satisfies a hop's entity-type guard.
///
/// An empty guard admits every linked entity; otherwise clause 4.9's
/// `1*(EntityType)` list is a disjunction, so one shared type is enough.
fn admitted_by_guard<'a, E: QueriedEntity<'a>>(linked: E, guard_types: &[EntityTypeName]) -> bool {
    guard_types.is_empty() || linked.types().any(|declared| guard_types.contains(declared))
}

/// Whether `reaches` takes some instance the sub-attribute chain
/// `sub_attributes` reaches from one attribute instance, one per instance of
/// each sub-attribute on the way, in the order the chain reaches them.
///
/// Clause 4.9: "If a Property has multiple instances (identified by its
/// respective datasetId), and no datasetId is explicitly addressed, the target
/// value shall be any Value of such instances". The `q` grammar addresses no
/// datasetId, so a sub-attribute's instances fan out exactly as an attribute's
/// do.
fn any_instance_along<'a, I: QueriedInstance<'a>>(instance: I, sub_attributes: &[AttributeName], reaches: &mut impl FnMut(I) -> bool) -> bool {
    match sub_attributes.split_first() {
        None => reaches(instance),
        Some((name, further)) => instance
            .sub_attribute_instances(name)
            .any(|reached| any_instance_along(reached, further, reaches)),
    }
}

/// Whether `accepts` takes a target of a local term's path in an entity:
/// every instance of the attribute, each sub-attribute instance on the way,
/// then the terminal.
///
/// All instances are visited, not just the first. Clause 4.9 (p. 86) defines
/// the comparison target for a multi-instance attribute as "any Value of such
/// instances", so a predicate holds when it holds for at least one of them,
/// whichever instance a `datasetId`-partitioned attribute writes first.
fn local_target<'a, E: QueriedEntity<'a>>(entity: E, term: &TermReading, accepts: &mut impl FnMut(ResolvedTarget<'a>) -> bool) -> bool {
    entity.attribute_instances(term.attribute()).any(|instance| {
        any_instance_along(instance, term.sub_attributes(), &mut |reached| {
            terminal_target(reached, term.terminal()).is_some_and(&mut *accepts)
        })
    })
}

/// Whether an instance's family keys its value by language, so a target
/// language selects strings of it.
fn keyed_by_language<'a, I: QueriedInstance<'a>>(instance: I) -> bool {
    family_reading(instance.family()).keys == ValueKeys::Languages
}

/// The target value `terminal` reads at one instance the path reaches, or
/// `None` when the instance holds none there.
///
/// A target language reads only a `LanguageProperty`, whose value is keyed by
/// language, and nothing on any other family.
fn terminal_target<'a, I: QueriedInstance<'a>>(instance: I, terminal: &Terminal) -> Option<ResolvedTarget<'a>> {
    match terminal {
        Terminal::OwnValue => family_target(instance),
        Terminal::InstanceMember(member) => Some(ResolvedTarget {
            value: TargetValue::of_metadata(*member, instance.metadata(*member))?,
            origin: TargetOrigin::AttributeValue,
            reading: MultiValueReading::WholeArray,
        }),
        Terminal::Language { language, matching } => keyed_by_language(instance).then(|| language_target(instance, language.selector(), *matching)),
        Terminal::Members(members) => resolve_compound_member(instance, members),
        Terminal::ByFamily { language, matching, members } => {
            if keyed_by_language(instance) {
                Some(language_target(instance, LanguageSelector::Tag(language), *matching))
            } else {
                resolve_compound_member(instance, members)
            }
        }
    }
}

/// Whether one instance the path reaches holds the element `terminal`
/// addresses.
///
/// With a target language the element is the key-value pair of that language
/// (clause 4.9), so it exists when a selected language holds a string.
fn terminal_exists<'a, I: QueriedInstance<'a>>(instance: I, terminal: &Terminal) -> bool {
    match terminal {
        Terminal::OwnValue => true,
        Terminal::InstanceMember(member) => instance.metadata(*member).next().is_some(),
        Terminal::Language { language, matching } => keyed_by_language(instance) && instance.language_values(language.selector(), *matching).next().is_some(),
        Terminal::Members(members) => holds_member(instance, members),
        Terminal::ByFamily { language, matching, members } => {
            if keyed_by_language(instance) {
                instance.language_values(LanguageSelector::Tag(language), *matching).next().is_some()
            } else {
                holds_member(instance, members)
            }
        }
    }
}

/// Whether an instance's compound value holds a value at `members`, as a
/// structured value's member or as a raw JSON object's key.
fn holds_member<'a, I: QueriedInstance<'a>>(instance: I, members: &MemberPath) -> bool {
    match family_reading(instance.family()).members {
        MemberSource::Geometry => geometry_member_target(instance, &members.written).is_some(),
        MemberSource::Value => {
            walk_member_hops(first_family_value(instance), &members.addressed).is_some()
                || family_target(instance).and_then(|target| json_member(&target, &members.written)).is_some()
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        q::attribute_axis::{addresses_element, any_target},
        queried_entity::{
            expanded_fixture::{FixtureEntity, target_json},
            linked_entities::NoLinkedEntities,
        },
    };
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_query::filter::{
        language_matching::LanguageMatching,
        path::{AttrPath, QueryPath},
        predicate::Predicate,
        reading::{family_reading::TargetOrigin, term_reading::TermReading},
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

    /// The reading of the existence term over a local path, its target
    /// language selecting keys by `matching`.
    fn term_along(path: &AttrPath, matching: LanguageMatching) -> TermReading {
        TermReading::of(&Predicate::Exists(QueryPath::Local(path.clone())), matching)
    }

    /// The target values a path resolves to under `matching`, with their
    /// origins.
    fn targets_under(entity: &Value, path: &AttrPath, matching: LanguageMatching) -> Vec<(Value, TargetOrigin)> {
        let entity = FixtureEntity::from_expanded(entity);
        let mut targets = Vec::new();
        any_target(&entity, &term_along(path, matching), NoLinkedEntities, &mut |target| {
            targets.push((target_json(&target.value), target.origin));
            false
        });
        targets
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

    /// Clause 4.9 makes each name after the first "a (sub)Property or
    /// (sub)Relationship", and a family's value member is neither, so a
    /// segment naming it (`speed.value` once expanded) addresses no element
    /// of any family.
    #[test]
    fn a_segment_naming_the_value_member_addresses_nothing() {
        let entity = json!({
            "https://example.com/speed": [{"https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 80}, {"@value": 90}]}],
            "https://example.com/owner": [{"https://uri.etsi.org/ngsi-ld/hasObject": [{"@id": "urn:ngsi-ld:Person:1"}]}],
            "https://example.com/colors": [{"https://uri.etsi.org/ngsi-ld/hasValueList": [{"@list": [{"@value": "red"}]}]}]
        });
        let fixture = FixtureEntity::from_expanded(&entity);
        for (attribute, member) in [
            ("https://example.com/speed", "https://uri.etsi.org/ngsi-ld/hasValue"),
            ("https://example.com/owner", "https://uri.etsi.org/ngsi-ld/hasObject"),
            ("https://example.com/owner", "https://uri.etsi.org/ngsi-ld/hasValue"),
            ("https://example.com/colors", "https://uri.etsi.org/ngsi-ld/hasValueList"),
        ] {
            let path = through(attribute, &[member]);
            pretty_assertions::assert_eq!(targets_along(&entity, &path), Vec::new(), "{attribute}.{member}");
            assert!(
                !addresses_element(&fixture, &term_along(&path, LanguageMatching::TagEquality), NoLinkedEntities),
                "{attribute}.{member}"
            );
        }
    }

    /// Clause 4.9 enters a compound value through the trailing path, "enclosed
    /// in a single pair of square brackets", so a structured value's member is
    /// reached there and never through a `.` segment naming the value member.
    #[test]
    fn a_structured_value_member_is_reached_through_the_trailing_path_alone() {
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
            Vec::new()
        );
        pretty_assertions::assert_eq!(
            targets_along(&entity, &bracketed("https://example.com/address", "https://example.com/city")),
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
        let english = parse_attribute_path("\"https://example.com/name\"[en]").expect("a path");
        assert!(!addresses_element(
            &entity,
            &term_along(&english, LanguageMatching::TagEquality),
            NoLinkedEntities
        ));
        assert!(addresses_element(
            &entity,
            &term_along(&english, LanguageMatching::ExtendedFiltering),
            NoLinkedEntities
        ));
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
