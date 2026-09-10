//! Navigating the clause 4.9 attribute axis of an expanded JSON-LD entity down
//! to the target values it addresses.
//!
//! Clause 4.9 gives an attribute reference three axes, and this module walks all
//! three: the `.`-separated sub-attribute chain, the trailing `[…]` group that
//! addresses a member of a compound value, and the `rel{…}` hop that crosses a
//! Relationship into the entity it names. What the axis terminates at — the
//! target value each attribute family carries — is
//! [`target_value`](crate::filter::target_value)'s.
//!
//! An attribute may carry several instances (clause 4.5.5), and each of them
//! contributes its own target: clause 4.9 (p. 86) defines the comparison target
//! of a multi-instance attribute as "any Value of such instances".

use crate::filter::{
    path::{AttrPath, QueryPath},
    target_value::{MultiValueReading, ResolvedTarget, TargetOrigin, VALUE_KEYWORD, first_family_value, terminal_target},
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_vocabulary::expanded::properties::ENTITY;
use serde_json::{Map, Value};
use std::slice::from_ref;

/// The JSON-LD keyword an entity carries its types under.
const TYPE_KEYWORD: &str = "@type";

/// Resolve every value an attribute axis addresses, descending each
/// linked-entity hop into the entity it names.
///
/// A hop can only be followed when the document already carries the linked
/// entity: clause 4.5.23.2 `join=inline` nests it under the Relationship
/// instance's `entity` member, and that embedded copy is the only linked data
/// an in-memory evaluation has. Without it the hop resolves no target and the
/// predicate does not hold. That is not a weaker reading of clause 4.9 — for a
/// stored entity the SQL compilation is what answers a traversal filter, and it
/// reads the linked entity from the store. This evaluator only ever sees the one
/// document it is handed, whether that is a notification payload being matched
/// against a subscription or a residual filter finishing what a store declined.
#[must_use]
pub fn resolve_targets(obj: &Map<String, Value>, path: &QueryPath) -> Vec<ResolvedTarget> {
    match path {
        QueryPath::Local(local) => resolve_values(obj, local),
        QueryPath::Linked {
            relationship,
            guard_types,
            inner,
        } => resolve_across_hop(obj, relationship, guard_types, inner),
    }
}

/// Resolve the inner axis against every entity embedded under a Relationship's
/// instances, keeping only those the hop's type guard admits.
fn resolve_across_hop(obj: &Map<String, Value>, relationship: &AttributeName, guard_types: &[AttributeName], inner: &QueryPath) -> Vec<ResolvedTarget> {
    let Some(attribute) = obj.get(relationship.as_str()) else {
        return Vec::new();
    };
    instances_of(attribute)
        .iter()
        .flat_map(|instance| instance.get(ENTITY.as_str()).into_iter().flat_map(|embedded| instances_of(embedded).iter()))
        .filter_map(Value::as_object)
        .filter(|linked| admitted_by_guard(linked, guard_types))
        .flat_map(|linked| resolve_targets(linked, inner))
        .collect()
}

/// The instances a member holds: an expanded JSON-LD member is an array, while a
/// canonical-keyed Context Source property is the bare node.
fn instances_of(member: &Value) -> &[Value] {
    match member.as_array() {
        Some(instances) => instances.as_slice(),
        None => from_ref(member),
    }
}

/// Whether a linked entity satisfies a hop's entity-type guard.
///
/// An empty guard admits every linked entity; otherwise clause 4.9's
/// `1*(EntityType)` list is a disjunction, so one shared type is enough.
fn admitted_by_guard(linked: &Map<String, Value>, guard_types: &[AttributeName]) -> bool {
    if guard_types.is_empty() {
        return true;
    }
    let Some(declared) = linked.get(TYPE_KEYWORD) else {
        return false;
    };
    instances_of(declared)
        .iter()
        .filter_map(Value::as_str)
        .any(|declared| guard_types.iter().any(|guard| guard.as_str() == declared))
}

/// Resolve every target value an attribute path addresses in an expanded
/// JSON-LD entity object or a canonical-keyed Context Source property map.
///
/// Navigates `obj[attr]` -> each instance -> sub-attrs -> the family's value
/// member, then the optional value path for compound values.
///
/// All instances are returned, not just the first. Clause 4.9 (p. 86) defines
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
fn resolve_values(obj: &Map<String, Value>, path: &AttrPath) -> Vec<ResolvedTarget> {
    let Some(attribute) = obj.get(path.attr.as_str()) else {
        return Vec::new();
    };
    instances_of(attribute)
        .iter()
        .filter_map(|instance| resolve_instance_value(instance, path))
        .collect()
}

/// Resolve one attribute instance down to the target value the predicate
/// compares.
fn resolve_instance_value(instance: &Value, path: &AttrPath) -> Option<ResolvedTarget> {
    // Navigate sub-attributes (e.g. temperature.observedAt).
    let mut current = instance;
    for sub in &path.sub_attrs {
        current = current.get(sub.as_str())?.as_array()?.first()?;
    }

    if path.value_path.is_empty() {
        return terminal_target(current);
    }
    resolve_compound_member(current, path)
}

/// Resolve a compound value path (`address[city]`) inside one attribute instance.
///
/// A compound value reaches this evaluator in either of two genuinely different
/// JSON-LD shapes, and both are addressed by the same `q` path:
///
/// * A structured Property value is expanded by JSON-LD into IRI-keyed member
///   nodes hanging off `hasValue`, so `city` is a member *hop* — `hasValue[0]`
///   then the member's own array, then that node's `@value`. This is the shape a
///   notification payload and a stored entity carry, and the one an earlier
///   evaluator missed entirely: it read the terminal `@value` first and then
///   indexed into it, which for this shape resolves nothing.
/// * A `JsonProperty`'s value, and a Property value that survived expansion as an
///   opaque JSON object, sit *under* a single `@value` as raw JSON, where `city`
///   is an ordinary object key. The NGSI-LD data model puts no constraint on
///   those keys, so they are read verbatim.
///
/// The member hop is tried first, addressed by the term-expanded spelling
/// (`path.value_path`): it is the shape term expansion produced the path for,
/// and a raw JSON object cannot carry a member whose key is the expanded IRI
/// without the client having written that IRI. The raw-JSON fallback then
/// retries with the pre-expansion spelling (`path.value_path_compact`),
/// because a `JsonProperty`'s value — and a Property value that survived
/// expansion as an opaque object — is never touched by JSON-LD expansion: its
/// keys are exactly what the client wrote, not what the request `@context`
/// expands them to.
fn resolve_compound_member(instance: &Value, path: &AttrPath) -> Option<ResolvedTarget> {
    if let Some(target) = resolve_member_hops(instance, &path.value_path) {
        return Some(target);
    }
    let seed = terminal_target(instance)?;
    let mut value = &seed.value;
    for key in &path.value_path_compact {
        value = value.get(key.as_str())?;
    }
    // clone: the caller owns the resolved value, which outlives the borrow of `instance`
    Some(ResolvedTarget {
        value: value.clone(),
        origin: seed.origin,
        reading: seed.reading,
    })
}

/// Walk expanded member nodes from an attribute instance down to a leaf `@value`.
///
/// Each hop is `node[<member IRI>][0]`, mirroring the jsonpath the SQL fallback
/// emits, and the leaf is read the same way an attribute's own value is.
fn resolve_member_hops(instance: &Value, value_path: &[AttributeName]) -> Option<ResolvedTarget> {
    let mut current = first_family_value(instance)?;
    for member in value_path {
        current = current.get(member.as_str())?.as_array()?.first()?;
    }
    let raw = current.get(VALUE_KEYWORD)?;
    // clone: the caller owns the resolved value, which outlives the borrow of `instance`
    Some(ResolvedTarget {
        value: raw.clone(),
        origin: TargetOrigin::AttributeValue,
        // A member of a compound value is one value, whatever the attribute
        // around it carries.
        reading: MultiValueReading::WholeArray,
    })
}

#[cfg(test)]
mod tests {
    use crate::filter::{
        attribute_axis::resolve_targets,
        path::{AttrPath, QueryPath},
    };
    use andromeda_ngsi_ld_vocabulary::expanded::properties::ENTITY;
    use serde_json::{Value, json};

    /// The target values one bare attribute path resolves to.
    fn targets_of(entity: &Value, attribute: &str) -> Vec<Value> {
        let obj = entity.as_object().expect("an expanded entity object");
        let path = QueryPath::Local(AttrPath::simple(attribute.parse().expect("a valid attribute name")));
        resolve_targets(obj, &path).into_iter().map(|target| target.value).collect()
    }

    #[test]
    fn the_embedded_entity_member_is_the_one_the_inline_join_renderer_writes() {
        // The renderer inserts the linked entity under the compacted `entity`
        // key; the core context expands that term to this IRI, which is what an
        // expanded document the evaluator sees carries.
        pretty_assertions::assert_eq!(ENTITY.as_str(), "https://uri.etsi.org/ngsi-ld/entity");
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

    /// A canonical-keyed Context Source property is the bare node rather than
    /// the one-element array an expanded member is (clause 5.2.9).
    #[test]
    fn a_context_source_property_resolves_without_the_expanded_array_wrapper() {
        let properties = json!({"https://example.com/sourceGroup": {"type": "Property", "value": "alpha"}});
        pretty_assertions::assert_eq!(targets_of(&properties, "https://example.com/sourceGroup"), vec![json!("alpha")]);
    }
}
