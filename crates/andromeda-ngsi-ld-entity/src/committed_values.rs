//! The spelling a write commits each value in, so the state a write commits is the state every later read decodes.
//!
//! - A JSON string declared as a clause 4.6.3 time value, an XSD number or an
//!   `xsd:boolean`, by its own `@type` or by the `valueType` of its attribute,
//!   is held in the one spelling the broker serves it in: a `DateTime` or a
//!   `Time` in UTC, `Z`, and the sub-second precision it carries in groups of
//!   three digits, a number in the canonical lexical form of its datatype, a
//!   Boolean as `true` or `false`. That holds for a single value, each value of
//!   a multi-valued Property, each `ListProperty` item and every typed literal
//!   nested in a structured value, at every depth. A `Date` has one spelling
//!   already. A string declared as none of them is never touched.
//! - A `ListRelationship` target is held in the clause 4.5.22.2 normalized
//!   spelling, `{"object": uri}`, whichever spelling the request used.

use crate::{
    attributes::Attributes,
    entity_state::EntityState,
    expanded_value::ExpandedValue,
    family_value::FamilyValue,
    instance::Instance,
    list_object::ListObject,
    literal::{Literal, LiteralValue},
    value_mode::ValueMode,
};
use andromeda_ngsi_ld_lexicon::{
    datatype::{declaration::LiteralDeclaration, declared_value::DeclaredValue},
    reference::NgsiLdReference,
    time::system_instant::SystemInstant,
};
use std::sync::Arc;

impl EntityState {
    /// Settle the state a write commits, given the entity before the write
    /// (`None` when the write creates it) and the write instant: the values
    /// take their committed spelling, then the attribute instances take their
    /// clause 4.8 system times
    /// ([`EntityState::stamp_attribute_instants`]). The spelling comes first,
    /// so restating a value in another spelling does not move its
    /// `modifiedAt`.
    pub fn settle(&mut self, previous: Option<&EntityState>, now: SystemInstant) {
        self.canonicalize_values(previous);
        self.stamp_attribute_instants(previous, now);
    }

    /// Give every value its committed spelling. An attribute this state still
    /// shares with `previous` was committed before and is left as it is, and
    /// an attribute is copied only when one of its values changes.
    pub fn canonicalize_values(&mut self, previous: Option<&EntityState>) {
        for (name, instances) in self.attributes_mut().iter_shared_mut() {
            let committed = previous
                .and_then(|previous| previous.attributes().get_shared(name))
                .is_some_and(|predecessors| Arc::ptr_eq(predecessors, instances));
            if committed || !instances.iter().any(|(_, instance)| instance_needs_canonical(instance)) {
                continue;
            }
            Arc::make_mut(instances).iter_mut().for_each(|(_, instance)| canonicalize_instance(instance));
        }
    }
}

impl Attributes {
    /// Give every value of these attributes, at every depth, its committed
    /// spelling, copying only the attributes whose values change.
    pub fn canonicalize_values(&mut self) {
        for (_, instances) in self.iter_shared_mut() {
            if instances.iter().any(|(_, instance)| instance_needs_canonical(instance)) {
                Arc::make_mut(instances).iter_mut().for_each(|(_, instance)| canonicalize_instance(instance));
            }
        }
    }
}

/// Whether a value of `instance`, or of one of its sub-attributes at any
/// depth, is not in its committed spelling.
fn instance_needs_canonical(instance: &Instance) -> bool {
    value_needs_canonical(instance.value())
        || instance
            .sub_attributes()
            .iter()
            .any(|(_, instances)| instances.iter().any(|(_, instance)| instance_needs_canonical(instance)))
}

/// Give the values of `instance` and of its sub-attributes their committed
/// spelling.
fn canonicalize_instance(instance: &mut Instance) {
    if value_needs_canonical(instance.value()) {
        canonicalize_value(instance.value_mut());
    }
    instance.sub_attributes_mut().canonicalize_values();
}

/// Whether `value` is not in its committed spelling.
fn value_needs_canonical(value: &FamilyValue) -> bool {
    match value {
        FamilyValue::Property { values, value_type, .. } | FamilyValue::ListProperty { items: values, value_type, .. } => {
            values.iter().any(|value| expanded_needs_canonical(value, value_type.as_ref()))
        }
        FamilyValue::ListRelationship { objects, .. } => objects.iter().any(|object| !matches!(object, ListObject::Wrapped(_))),
        FamilyValue::GeoProperty { .. }
        | FamilyValue::Relationship { .. }
        | FamilyValue::LanguageProperty { .. }
        | FamilyValue::VocabProperty { .. }
        | FamilyValue::JsonProperty { .. } => false,
    }
}

/// Rewrite `value` in its committed spelling.
fn canonicalize_value(value: &mut FamilyValue) {
    match value {
        FamilyValue::Property { values, value_type, .. } | FamilyValue::ListProperty { items: values, value_type, .. } => {
            for value in values.iter_mut() {
                value.commit_declared_values(value_type.as_ref());
            }
        }
        FamilyValue::ListRelationship { objects, .. } => {
            for object in objects.iter_mut() {
                if let ListObject::Bare(uri) | ListObject::Literal(uri) = object {
                    // clone: the target moves from one spelling to the other.
                    *object = ListObject::Wrapped(uri.clone());
                }
            }
        }
        FamilyValue::GeoProperty { .. }
        | FamilyValue::Relationship { .. }
        | FamilyValue::LanguageProperty { .. }
        | FamilyValue::VocabProperty { .. }
        | FamilyValue::JsonProperty { .. } => {}
    }
}

/// Whether `value`, or a literal inside it, is not in its committed spelling.
/// `value_type` declares a primitive member, a list item included, and nothing
/// inside a structured value.
fn expanded_needs_canonical<M: ValueMode>(value: &ExpandedValue<M>, value_type: Option<&NgsiLdReference>) -> bool {
    match value {
        ExpandedValue::Literal(literal) => canonical_literal(literal, value_type).is_some(),
        ExpandedValue::Json(_) => false,
        ExpandedValue::List(items) => items.iter().any(|item| expanded_needs_canonical(item, value_type)),
        ExpandedValue::Node(node) => node
            .members()
            .values()
            .any(|member| M::held_values(member).iter().any(|item| expanded_needs_canonical(item, None))),
    }
}

impl<M: ValueMode> ExpandedValue<M> {
    /// Rewrite this value, beside the `valueType` `value_type`, and every
    /// literal inside it in its committed spelling: a declared time value,
    /// number or Boolean takes its served spelling, every other literal is left
    /// as it is.
    ///
    /// The Temporal API records written values without a commit, so a Temporal
    /// Evolution holds a declared value in the same spelling current state
    /// does.
    pub fn commit_declared_values(&mut self, value_type: Option<&NgsiLdReference>) {
        match self {
            ExpandedValue::Literal(literal) => {
                if let Some(canonical) = canonical_literal(literal, value_type) {
                    *literal = canonical;
                }
            }
            ExpandedValue::Json(_) => {}
            ExpandedValue::List(items) => items.iter_mut().for_each(|item| item.commit_declared_values(value_type)),
            ExpandedValue::Node(node) => node
                .members_mut()
                .values_mut()
                .for_each(|member| M::held_values_mut(member).iter_mut().for_each(|item| item.commit_declared_values(None))),
        }
    }
}

/// The literal a declared value is committed as, when `literal` is one
/// written in another spelling.
fn canonical_literal(literal: &Literal, value_type: Option<&NgsiLdReference>) -> Option<Literal> {
    let LiteralValue::String(text) = literal.value() else {
        return None;
    };
    let canonical = match DeclaredValue::of(LiteralDeclaration::new(literal.datatype(), value_type), literal.value().primitive()) {
        Some(Ok(value)) => value.to_string(),
        Some(Err(_)) | None => return None,
    };
    (canonical != text.as_str()).then(|| literal.respelled(canonical.into())).flatten()
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        expanded_value::{ExpandedValue, ValueNode},
        family_value::FamilyValue,
        instance::Instance,
        list_object::ListObject,
        literal::{Literal, LiteralValue},
        operation::speed_entity::{SPEED, entity, instances, member_name, name, property, text},
    };
    use andromeda_ngsi_ld_lexicon::{reference::NgsiLdReference, time::system_instant::SystemInstant, uri::Uri};
    use andromeda_ngsi_ld_vocabulary::expanded::classes;
    use chrono::{DateTime, Duration};
    use indexmap::IndexMap;
    use std::sync::Arc;

    fn date_time_type() -> NgsiLdReference {
        classes::DATE_TIME.as_str().parse().expect("the DateTime datatype")
    }

    fn time_type() -> NgsiLdReference {
        classes::TIME.as_str().parse().expect("the Time datatype")
    }

    fn typed(value: &str, datatype: NgsiLdReference) -> ExpandedValue {
        ExpandedValue::Literal(Literal::typed(LiteralValue::String(value.into()), datatype))
    }

    fn declared_date_time(value: &str) -> Instance {
        property(typed(value, date_time_type()))
    }

    fn several(values: Vec<ExpandedValue>, value_type: Option<NgsiLdReference>) -> Instance {
        Instance::builder()
            .value(FamilyValue::Property {
                values,
                unit_code: None,
                value_type,
            })
            .build()
    }

    fn list_property(items: Vec<ExpandedValue>) -> Instance {
        Instance::builder()
            .value(FamilyValue::ListProperty {
                items,
                unit_code: None,
                value_type: None,
            })
            .build()
    }

    fn node(member: &str, values: Vec<ExpandedValue>) -> ExpandedValue {
        ExpandedValue::Node(Box::new(ValueNode::new(None, Vec::new(), IndexMap::from([(member_name(member), values)]))))
    }

    /// The instance the `speed` attribute of `state` holds once committed.
    fn committed(instance: Instance) -> Instance {
        let mut state = entity(vec![(SPEED, instances(vec![(None, instance)]))]);
        state.canonicalize_values(None);
        let held = state.attributes().get(SPEED).and_then(|held| held.get(None)).expect("the instance");
        held.clone()
    }

    fn uri(raw: &str) -> Uri {
        raw.parse().expect("a URI")
    }

    #[test]
    fn a_typed_date_time_is_committed_in_its_served_spelling() {
        pretty_assertions::assert_eq!(
            committed(declared_date_time("2024-01-01T00:00:00,5Z")),
            declared_date_time("2024-01-01T00:00:00.500Z")
        );
    }

    #[test]
    fn a_typed_time_is_committed_in_its_served_spelling() {
        pretty_assertions::assert_eq!(
            committed(property(typed("12:00:00.000Z", time_type()))),
            property(typed("12:00:00Z", time_type()))
        );
    }

    /// Annex C.6: a string declared as no time value is the string it is.
    #[test]
    fn an_untyped_string_in_a_time_value_form_is_committed_as_written() {
        for written in ["2024-01-01T00:00:00.250000Z", "12:00:00.5Z", "2024-01-01T00:00:00,5Z"] {
            pretty_assertions::assert_eq!(committed(property(text(written))), property(text(written)), "{written}");
        }
    }

    #[test]
    fn every_typed_member_of_several_values_is_committed_in_its_served_spelling() {
        pretty_assertions::assert_eq!(
            committed(several(vec![typed("2024-01-01T00:00:00.000Z", date_time_type()), text("x")], None)),
            several(vec![typed("2024-01-01T00:00:00Z", date_time_type()), text("x")], None)
        );
    }

    #[test]
    fn every_typed_list_property_item_is_committed_in_its_served_spelling() {
        pretty_assertions::assert_eq!(
            committed(list_property(vec![typed("12:00:00.500000Z", time_type()), text("12:00:00.500000Z")])),
            list_property(vec![typed("12:00:00.500Z", time_type()), text("12:00:00.500000Z")])
        );
    }

    #[test]
    fn a_typed_date_time_inside_a_structured_value_is_committed_in_its_served_spelling() {
        let structured = |at: &str| {
            property(node(
                "https://example.org/reading",
                vec![node("https://example.org/at", vec![typed(at, date_time_type())])],
            ))
        };
        pretty_assertions::assert_eq!(committed(structured("2024-01-01T00:00:00,25Z")), structured("2024-01-01T00:00:00.250Z"));
    }

    /// Annex C.6: `valueType` declares the datatype of the plain value it sits
    /// beside, which stays plain.
    #[test]
    fn a_value_type_date_time_is_committed_plain_in_its_served_spelling() {
        pretty_assertions::assert_eq!(
            committed(several(vec![text("2024-01-01T00:00:00.000Z")], Some(date_time_type()))),
            several(vec![text("2024-01-01T00:00:00Z")], Some(date_time_type()))
        );
    }

    fn xsd(local: &str) -> NgsiLdReference {
        format!("http://www.w3.org/2001/XMLSchema#{local}").parse().expect("an XSD datatype")
    }

    /// XML Schema Definition Language 1.1 Part 2: the canonical lexical form of
    /// a decimal has no `+`, no leading and no trailing fractional zeros, and
    /// that of a Boolean is `true` or `false`.
    #[test]
    fn a_declared_number_or_boolean_is_committed_in_its_canonical_spelling() {
        for (written, canonical, datatype) in [
            ("01", "1", xsd("integer")),
            ("+1.50", "1.5", xsd("decimal")),
            ("1.0E2", "100", xsd("double")),
            ("0.0000001", "1E-7", xsd("double")),
            ("1", "true", xsd("boolean")),
            ("INF", "INF", xsd("double")),
        ] {
            pretty_assertions::assert_eq!(
                committed(property(typed(written, datatype.clone()))),
                property(typed(canonical, datatype.clone())),
                "{written}"
            );
            pretty_assertions::assert_eq!(
                committed(several(vec![text(written)], Some(datatype.clone()))),
                several(vec![text(canonical)], Some(datatype)),
                "{written} by valueType"
            );
        }
    }

    #[test]
    fn a_string_declared_as_another_xsd_datatype_is_committed_as_written() {
        pretty_assertions::assert_eq!(committed(property(typed("01", xsd("string")))), property(typed("01", xsd("string"))));
    }

    #[test]
    fn a_sub_attribute_date_time_is_committed_in_its_served_spelling() {
        let mut parent = property(text("x"));
        parent
            .sub_attributes_mut()
            .insert(name(SPEED), instances(vec![(None, declared_date_time("2024-01-01T00:00:00.5Z"))]));
        let held = committed(parent);
        let child = held.sub_attributes().get(SPEED).and_then(|held| held.get(None)).expect("the child");
        pretty_assertions::assert_eq!(child, &declared_date_time("2024-01-01T00:00:00.500Z"));
    }

    #[test]
    fn list_relationship_targets_are_committed_wrapped() {
        let list = |objects| {
            Instance::builder()
                .value(FamilyValue::ListRelationship {
                    objects,
                    object_types: Vec::new(),
                })
                .build()
        };
        pretty_assertions::assert_eq!(
            committed(list(vec![
                ListObject::Bare(uri("urn:a")),
                ListObject::Literal(uri("urn:b")),
                ListObject::Wrapped(uri("urn:c")),
            ])),
            list(vec![
                ListObject::Wrapped(uri("urn:a")),
                ListObject::Wrapped(uri("urn:b")),
                ListObject::Wrapped(uri("urn:c"))
            ])
        );
    }

    #[test]
    fn an_attribute_shared_with_the_previous_state_is_not_copied() {
        let previous = entity(vec![(SPEED, instances(vec![(None, declared_date_time("2024-01-01T00:00:00.5Z"))]))]);
        let mut state = previous.clone();
        state.canonicalize_values(Some(&previous));
        let (Some(before), Some(after)) = (previous.attributes().get_shared(SPEED), state.attributes().get_shared(SPEED)) else {
            panic!("the attribute in both states");
        };
        assert!(Arc::ptr_eq(before, after));
    }

    #[test]
    fn restating_a_typed_date_time_in_another_spelling_keeps_its_modified_at() {
        let first = SystemInstant::new(DateTime::UNIX_EPOCH);
        let later = SystemInstant::new(DateTime::UNIX_EPOCH + Duration::days(1));
        let mut previous = entity(vec![(SPEED, instances(vec![(None, declared_date_time("2024-01-01T00:00:00.500Z"))]))]);
        previous.settle(None, first);
        let mut restated = entity(vec![(SPEED, instances(vec![(None, declared_date_time("2024-01-01T00:00:00.5Z"))]))]);
        restated.settle(Some(&previous), later);
        pretty_assertions::assert_eq!(restated, previous);
    }

    #[test]
    fn restating_an_untyped_string_in_another_spelling_is_a_modification() {
        let first = SystemInstant::new(DateTime::UNIX_EPOCH);
        let later = SystemInstant::new(DateTime::UNIX_EPOCH + Duration::days(1));
        let mut previous = entity(vec![(SPEED, instances(vec![(None, property(text("2024-01-01T00:00:00.500Z")))]))]);
        previous.settle(None, first);
        let mut restated = entity(vec![(SPEED, instances(vec![(None, property(text("2024-01-01T00:00:00.5Z")))]))]);
        restated.settle(Some(&previous), later);
        assert_ne!(restated, previous);
    }
}
