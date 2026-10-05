//! Navigating the clause 4.9 attribute axis of an entity down to the target
//! values it addresses.
//!
//! Clause 4.9 gives an attribute reference three axes, and this module walks all
//! three: the `.`-separated sub-attribute chain, the trailing `[…]` group that
//! addresses a member of a compound value or the target language of a
//! `LanguageProperty`, and the `rel{…}` hop that crosses a Relationship into the
//! entity it names. What the axis terminates at — the target value each
//! attribute family carries — is
//! [`family_target`](crate::filter::family_target)'s.
//!
//! An attribute may carry several instances (clause 4.5.5), and each of them
//! contributes its own target: clause 4.9 (p. 86) defines the comparison target
//! of a multi-instance attribute as "any Value of such instances".

use crate::{
    filter::{
        family_target::{TerminalKeyword, family_target, first_family_value},
        path::{AttrPath, QueryPath},
        target_value::{MultiValueReading, ResolvedTarget, TargetOrigin, TargetValue, json_member},
    },
    queried_entity::{
        attribute_reading::{QueriedEntity, QueriedInstance, QueriedValueNode},
        instance_metadata::InstanceMetadata,
        linked_entities::LinkedEntities,
        value_item::ValueItem,
    },
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType};
use std::borrow::Cow;

/// Resolve every value an attribute axis addresses, descending each
/// linked-entity hop into the entity it names in `linked`.
///
/// Clause 4.9 EXAMPLE 13: a hop "makes a sub-query on matching target objects",
/// the entities the Relationship's objects name. The entity under evaluation
/// names them and `linked` holds them; an object `linked` does not hold
/// reaches nothing.
#[must_use]
pub fn resolve_targets<'a, E: QueriedEntity<'a>, L: LinkedEntities<'a>>(entity: E, path: &QueryPath, linked: L) -> Vec<ResolvedTarget<'a>> {
    match path {
        QueryPath::Local(local) => resolve_values(entity, local),
        QueryPath::Linked {
            relationship,
            guard_types,
            inner,
        } => relationship_objects(entity, relationship)
            .filter_map(|object| linked.linked(object))
            .filter(|target| admitted_by_guard(*target, guard_types))
            .flat_map(|target| resolve_targets(target, inner, linked))
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
pub fn addresses_element<'a, E: QueriedEntity<'a>, L: LinkedEntities<'a>>(entity: E, path: &QueryPath, linked: L) -> bool {
    match path {
        QueryPath::Local(local) => entity.attribute_instances(&local.attr).any(|instance| instance_holds_element(instance, local)),
        QueryPath::Linked {
            relationship,
            guard_types,
            inner,
        } => relationship_objects(entity, relationship)
            .filter_map(|object| linked.linked(object))
            .filter(|target| admitted_by_guard(*target, guard_types))
            .any(|target| addresses_element(target, inner, linked)),
    }
}

/// Whether one attribute instance holds the element a path addresses.
///
/// With a target language the element is the key-value pair of that language
/// (clause 4.9), so it exists when the language holds a string.
fn instance_holds_element<'a, I: QueriedInstance<'a>>(instance: I, path: &AttrPath) -> bool {
    let Some(position) = path.sub_attrs.iter().try_fold(AxisPosition::Instance(instance), AxisPosition::step) else {
        return false;
    };
    if path.value_path.is_empty() {
        return true;
    }
    if let Some((language_instance, language)) = position.target_language(path) {
        return language_instance.language_values(language).next().is_some();
    }
    walk_member_hops(position.first_family_value(), &path.value_path).is_some()
        || position.terminal().and_then(|target| json_member(&target, &path.value_path_compact)).is_some()
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
/// values, then the optional value path for compound values.
///
/// All instances are resolved, not just the first. Clause 4.9 (p. 86) defines
/// the comparison target for a multi-instance attribute as "any Value of such
/// instances", so a predicate holds when it holds for at least one of them.
/// Restricting this to the first instance made a `datasetId`-partitioned
/// attribute match on whichever instance happened to serialise first.
///
/// The `[*]` language-suppression marker selects no different resolution here.
/// Clause 4.9 defines a `LanguageProperty`'s target value with no target
/// language as "a value from any of the key-value pairs held within the
/// languageMap", which is the one array target the unqualified path already
/// addresses — the same reading the SQL compilation takes of the marker.
fn resolve_values<'a, E: QueriedEntity<'a>>(entity: E, path: &AttrPath) -> Vec<ResolvedTarget<'a>> {
    entity
        .attribute_instances(&path.attr)
        .filter_map(|instance| resolve_instance_value(instance, path))
        .collect()
}

/// Resolve one attribute instance down to the target value the predicate
/// compares.
fn resolve_instance_value<'a, I: QueriedInstance<'a>>(instance: I, path: &AttrPath) -> Option<ResolvedTarget<'a>> {
    let mut position = AxisPosition::Instance(instance);
    for name in &path.sub_attrs {
        position = position.step(name)?;
    }
    if path.value_path.is_empty() {
        return position.terminal();
    }
    if let Some((language_instance, language)) = position.target_language(path) {
        return language_target(language_instance, language);
    }
    resolve_compound_member(position, path)
}

/// The target value clause 4.9 defines for a `LanguageProperty` with a target
/// language: "the Value associated to the matching key-value pair held within
/// the languageMap ... where the key matches the target language".
///
/// A language holding several strings is matched "as a single element from the
/// array of values of the key-value pair", which is the any-one reading the
/// language map has with no target language. A language the map does not hold
/// is no target element at all, so no operator — `!=` and `!~=` included —
/// matches it.
fn language_target<'a, I: QueriedInstance<'a>>(instance: I, language: &str) -> Option<ResolvedTarget<'a>> {
    let mut strings = instance.language_values(language).filter_map(|item| TerminalKeyword::Literal.read(item));
    let first = strings.next()?;
    let value = match strings.next() {
        None => first,
        Some(second) => TargetValue::Array([first, second].into_iter().chain(strings).collect()),
    };
    Some(ResolvedTarget {
        value,
        origin: TargetOrigin::AttributeValue,
        reading: MultiValueReading::AnyOneValue,
    })
}

/// Where a walk down the `.` axis stands.
///
/// A `.` segment names a sub-attribute, and the walk stands on its first
/// instance. It may instead name a member the instance carries as data of its
/// own: an [`InstanceMetadata`] member, which ends the walk on its lexical
/// form, or the family's value member itself (`speed.value`), which lands on
/// the first value. From a structured value the walk continues into its
/// members.
enum AxisPosition<'a, I: QueriedInstance<'a>> {
    /// An attribute or sub-attribute instance.
    Instance(I),
    /// One value of a family member or of a structured value's member.
    Value(ValueItem<'a, I::Node>),
    /// The lexical form of an instance member that is not a sub-attribute.
    Metadata(Cow<'a, str>),
}

impl<'a, I: QueriedInstance<'a>> AxisPosition<'a, I> {
    /// The position one `.` segment further down, or `None` when the segment
    /// names nothing there.
    fn step(self, name: &AttributeName) -> Option<AxisPosition<'a, I>> {
        match self {
            AxisPosition::Instance(instance) => {
                if let Some(member) = InstanceMetadata::named(name) {
                    return instance.metadata(member).map(AxisPosition::Metadata);
                }
                if instance.family().value_key().as_str() == name.as_str() {
                    return first_family_value(instance).map(AxisPosition::Value);
                }
                instance.sub_attribute(name).map(AxisPosition::Instance)
            }
            AxisPosition::Value(ValueItem::Node(node)) => node.first_member_value(name).map(AxisPosition::Value),
            AxisPosition::Value(ValueItem::Literal(_) | ValueItem::Json(_) | ValueItem::Reference(_) | ValueItem::Opaque) | AxisPosition::Metadata(_) => None,
        }
    }

    /// The `LanguageProperty` instance at this position with the target
    /// language `path`'s trailing group names, or `None` when the trailing
    /// group addresses a compound member instead.
    ///
    /// Clause 4.9 gives `attr[member]` two readings, and the target element
    /// decides which applies: a `LanguageProperty`'s value is a language map,
    /// whose one-member trailing group is the target language (EXAMPLE
    /// `color[en]=="red"`); every other family's is a compound member. The
    /// language is the member as the client wrote it, because a language tag
    /// is never a term a `@context` expands.
    fn target_language<'p>(&self, path: &'p AttrPath) -> Option<(I, &'p str)> {
        match (self, path.value_path_compact.as_slice()) {
            (AxisPosition::Instance(instance), [language]) if instance.family() == AttributeType::LanguageProperty => Some((*instance, language.as_str())),
            (AxisPosition::Instance(_) | AxisPosition::Value(_) | AxisPosition::Metadata(_), _) => None,
        }
    }

    /// The first value of the family member at this position, the node a
    /// compound value's members hang off.
    fn first_family_value(&self) -> Option<ValueItem<'a, I::Node>> {
        match self {
            AxisPosition::Instance(instance) => first_family_value(*instance),
            AxisPosition::Value(_) | AxisPosition::Metadata(_) => None,
        }
    }

    /// The target value this position holds, or `None` when it holds none.
    fn terminal(self) -> Option<ResolvedTarget<'a>> {
        let value = match self {
            AxisPosition::Instance(instance) => return family_target(instance),
            AxisPosition::Value(item) => TerminalKeyword::Identifier.read(item)?,
            AxisPosition::Metadata(text) => TargetValue::Text(text),
        };
        Some(ResolvedTarget {
            value,
            origin: TargetOrigin::AttributeValue,
            reading: MultiValueReading::WholeArray,
        })
    }
}

/// Resolve a compound value path (`address[city]`) at one position of the
/// axis.
///
/// A compound value reaches this evaluator in either of two genuinely different
/// shapes, and both are addressed by the same `q` path:
///
/// * A structured Property value is a node whose members are keyed by expanded
///   IRI, so `city` is a member *hop* — the first value, then the member's
///   first value, down to a literal. This is the shape a notification payload
///   and a stored entity carry, and the one an earlier evaluator missed
///   entirely: it read the terminal literal first and then indexed into it,
///   which for this shape resolves nothing.
/// * A `JsonProperty`'s value, and a Property value held as an opaque JSON
///   object, are raw JSON, where `city` is an ordinary object key. The NGSI-LD
///   data model puts no constraint on those keys, so they are read verbatim.
///
/// The member hop is tried first, addressed by the term-expanded spelling
/// (`path.value_path`): it is the shape term expansion produced the path for,
/// and a raw JSON object cannot carry a member whose key is the expanded IRI
/// without the client having written that IRI. The raw-JSON fallback then
/// retries with the pre-expansion spelling (`path.value_path_compact`),
/// because raw JSON is never touched by JSON-LD expansion: its keys are exactly
/// what the client wrote, not what the request `@context` expands them to.
fn resolve_compound_member<'a, I: QueriedInstance<'a>>(position: AxisPosition<'a, I>, path: &AttrPath) -> Option<ResolvedTarget<'a>> {
    if let Some(target) = resolve_member_hops(position.first_family_value(), &path.value_path) {
        return Some(target);
    }
    json_member(&position.terminal()?, &path.value_path_compact)
}

/// Walk the members of a structured value down to a literal.
///
/// Each hop is the member's first value, mirroring the jsonpath the SQL
/// fallback emits, and the leaf is read as a literal.
fn resolve_member_hops<'a, N: QueriedValueNode<'a>>(start: Option<ValueItem<'a, N>>, value_path: &[AttributeName]) -> Option<ResolvedTarget<'a>> {
    Some(ResolvedTarget {
        value: TerminalKeyword::Literal.read(walk_member_hops(start, value_path)?)?,
        origin: TargetOrigin::AttributeValue,
        // A member of a compound value is one value, whatever the attribute
        // around it carries.
        reading: MultiValueReading::WholeArray,
    })
}

/// The value a structured value's member path leads to, one member's first
/// value per hop, or `None` when a hop finds no member to descend into.
fn walk_member_hops<'a, N: QueriedValueNode<'a>>(start: Option<ValueItem<'a, N>>, value_path: &[AttributeName]) -> Option<ValueItem<'a, N>> {
    value_path.iter().try_fold(start?, |current, member| match current {
        ValueItem::Node(node) => node.first_member_value(member),
        ValueItem::Literal(_) | ValueItem::Json(_) | ValueItem::Reference(_) | ValueItem::Opaque => None,
    })
}

#[cfg(test)]
mod tests {
    use crate::{
        filter::{
            attribute_axis::resolve_targets,
            path::{AttrPath, QueryPath},
            target_value::TargetOrigin,
        },
        queried_entity::{
            expanded_fixture::{FixtureEntity, target_json},
            linked_entities::NoLinkedEntities,
        },
    };
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use serde_json::{Value, json};

    /// An attribute name for a test path.
    fn name(raw: &str) -> AttributeName {
        raw.parse().expect("a valid attribute name")
    }

    /// The target values a path resolves to, with their origins.
    fn targets_along(entity: &Value, path: &AttrPath) -> Vec<(Value, TargetOrigin)> {
        let entity = FixtureEntity::from_expanded(entity);
        resolve_targets(&entity, &QueryPath::Local(path.clone()), NoLinkedEntities) // clone: the test owns the path it wraps
            .into_iter()
            .map(|target| (target_json(&target.value), target.origin))
            .collect()
    }

    /// A path whose trailing group is the one member `member`, as the client
    /// spelled it.
    fn bracketed(attribute: &str, member: &str) -> AttrPath {
        let mut path = AttrPath::simple(name(attribute));
        path.value_path = vec![name(member)];
        path.value_path_compact = vec![name(member)];
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
        pretty_assertions::assert_eq!(targets_along(&entity, &bracketed("https://example.com/name", "fr")), Vec::new());
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

    /// A sub-attribute chain reads the first instance of each sub-attribute.
    #[test]
    fn a_sub_attribute_chain_reads_the_first_instance_of_each_hop() {
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
            vec![(json!("urn:ngsi-ld:Sensor:1"), TargetOrigin::RelationshipObject)]
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
        let mut path = AttrPath::simple(name("https://example.com/myJson"));
        path.value_path = vec![name("https://example.com/a"), name("https://example.com/b")];
        path.value_path_compact = vec![name("a"), name("b")];
        pretty_assertions::assert_eq!(targets_along(&entity, &path), vec![(json!([1, 2]), TargetOrigin::AttributeValue)]);
        path.value_path_compact = vec![name("a"), name("b"), name("c")];
        pretty_assertions::assert_eq!(targets_along(&entity, &path), Vec::new());
    }
}
