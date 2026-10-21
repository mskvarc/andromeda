//! The spellings a write commits in place of the ones it was given.
//!
//! Storage keeps some values by what they mean rather than by how they were
//! spelled, so a write commits them in the one spelling a read gives back, and
//! the state a write commits is the state every later read decodes:
//!
//! - A Property holding one clause 4.6.3 `DateTime`, declared or read off its
//!   lexical form, is held as its instant in the rendering the broker gives
//!   every instant: UTC, `Z`, and the sub-second precision the instant carries
//!   in groups of three digits. Clause 4.6.4 asks the broker to preserve the
//!   content of values, not the spelling of a timestamp.
//! - An `observedAt` and an `expiresAt`, of the Entity or of an instance at
//!   any depth, are held in the same rendering. Admission reads them as
//!   written first, which is what refuses an `observedAt` outside the clause
//!   4.6.3 UTC form.
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
};
use andromeda_ngsi_ld_lexicon::{
    reference::NgsiLdReference,
    time::{rendering::instant_text, system_instant::SystemInstant, value::TimeValue, written_date_time::DateTimeText},
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
        if let Some(canonical) = self.expires_at().and_then(canonical_instant) {
            self.set_expires_at(Some(canonical));
        }
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
        || instance.observed_at().and_then(canonical_instant).is_some()
        || instance.expires_at().and_then(canonical_instant).is_some()
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
    if let Some(canonical) = instance.observed_at().and_then(canonical_instant) {
        instance.set_observed_at(Some(canonical));
    }
    if let Some(canonical) = instance.expires_at().and_then(canonical_instant) {
        instance.set_expires_at(Some(canonical));
    }
    instance.sub_attributes_mut().canonicalize_values();
}

/// Whether `value` is not in its committed spelling.
fn value_needs_canonical(value: &FamilyValue) -> bool {
    match value {
        FamilyValue::Property { values, .. } => match values.as_slice() {
            [ExpandedValue::Literal(literal)] => canonical_date_time(literal).is_some(),
            [ExpandedValue::Json(_) | ExpandedValue::Node(_) | ExpandedValue::List(_)] | [] | [_, _, ..] => false,
        },
        FamilyValue::ListRelationship { objects, .. } => objects.iter().any(|object| !matches!(object, ListObject::Wrapped(_))),
        FamilyValue::GeoProperty { .. }
        | FamilyValue::Relationship { .. }
        | FamilyValue::LanguageProperty { .. }
        | FamilyValue::VocabProperty { .. }
        | FamilyValue::JsonProperty { .. }
        | FamilyValue::ListProperty { .. } => false,
    }
}

/// Rewrite `value` in its committed spelling.
fn canonicalize_value(value: &mut FamilyValue) {
    match value {
        FamilyValue::Property { values, .. } => {
            if let [ExpandedValue::Literal(literal)] = values.as_mut_slice()
                && let Some(canonical) = canonical_date_time(literal)
            {
                *literal = canonical;
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
        | FamilyValue::JsonProperty { .. }
        | FamilyValue::ListProperty { .. } => {}
    }
}

/// The member a `DateTime` member is committed as, when `text` spells its
/// instant another way.
fn canonical_instant(text: &DateTimeText) -> Option<DateTimeText> {
    let canonical = DateTimeText::from_instant(text.instant());
    (canonical != *text).then_some(canonical)
}

/// The literal a clause 4.6.3 `DateTime` is committed as, when `literal` is
/// one written in another spelling.
fn canonical_date_time(literal: &Literal) -> Option<Literal> {
    let LiteralValue::String(text) = literal.value() else {
        return None;
    };
    let instant = match TimeValue::admitted(literal.datatype().map(NgsiLdReference::as_str), text) {
        Some(Ok(TimeValue::DateTime(instant))) => instant.get(),
        Some(Ok(TimeValue::Date(_) | TimeValue::Time(_)) | Err(_)) | None => return None,
    };
    let canonical = instant_text(instant);
    if canonical == text.as_str() {
        return None;
    }
    literal.respelled(canonical.into())
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_state::EntityState,
        expanded_value::ExpandedValue,
        family_value::FamilyValue,
        instance::Instance,
        list_object::ListObject,
        literal::{Literal, LiteralValue},
        operation::speed_entity::{SPEED, entity, instances, name, property, text},
    };
    use andromeda_ngsi_ld_lexicon::{
        reference::NgsiLdReference,
        time::{system_instant::SystemInstant, written_date_time::DateTimeText},
        uri::Uri,
    };
    use andromeda_ngsi_ld_vocabulary::expanded::classes;
    use chrono::{DateTime, Duration};
    use std::sync::Arc;

    /// The `DateTime` datatype IRI.
    fn date_time_type() -> NgsiLdReference {
        classes::DATE_TIME.as_str().parse().expect("the DateTime datatype")
    }

    /// A Property holding one literal typed `DateTime`.
    fn declared_date_time(value: &str) -> Instance {
        property(ExpandedValue::Literal(Literal::typed(LiteralValue::String(value.into()), date_time_type())))
    }

    /// The single literal the `speed` Property of `state` holds.
    fn held_literal(state: &EntityState) -> Literal {
        let instances = state.attributes().get(SPEED).expect("the attribute");
        let (_, instance) = instances.iter().next().expect("one instance");
        let FamilyValue::Property { values, .. } = instance.value() else {
            panic!("a Property expected");
        };
        let [ExpandedValue::Literal(literal)] = values.as_slice() else {
            panic!("one literal expected");
        };
        literal.clone()
    }

    /// A URI.
    fn uri(raw: &str) -> Uri {
        raw.parse().expect("a URI")
    }

    #[test]
    fn a_declared_date_time_is_committed_in_its_rendered_spelling() {
        let mut state = entity(vec![(SPEED, instances(vec![(None, declared_date_time("2024-01-01T00:00:00,5Z"))]))]);
        state.canonicalize_values(None);
        pretty_assertions::assert_eq!(
            held_literal(&state),
            Literal::typed(LiteralValue::String("2024-01-01T00:00:00.500Z".into()), date_time_type())
        );
    }

    #[test]
    fn a_lexical_date_time_keeps_no_datatype() {
        let mut state = entity(vec![(SPEED, instances(vec![(None, property(text("2024-01-01T00:00:00.000Z")))]))]);
        state.canonicalize_values(None);
        pretty_assertions::assert_eq!(held_literal(&state), Literal::plain(LiteralValue::String("2024-01-01T00:00:00Z".into())));
    }

    #[test]
    fn a_time_keeps_its_lexical_text() {
        let mut state = entity(vec![(SPEED, instances(vec![(None, property(text("12:00:00.5Z")))]))]);
        state.canonicalize_values(None);
        pretty_assertions::assert_eq!(held_literal(&state), Literal::plain(LiteralValue::String("12:00:00.5Z".into())));
    }

    #[test]
    fn a_date_time_inside_several_values_keeps_its_spelling() {
        let several = Instance::builder()
            .value(FamilyValue::Property {
                values: vec![text("2024-01-01T00:00:00.5Z"), text("x")],
                unit_code: None,
                value_type: None,
            })
            .build();
        let mut state = entity(vec![(SPEED, instances(vec![(None, several.clone())]))]);
        state.canonicalize_values(None);
        pretty_assertions::assert_eq!(state, entity(vec![(SPEED, instances(vec![(None, several)]))]));
    }

    #[test]
    fn a_sub_attribute_date_time_is_committed_in_its_rendered_spelling() {
        let mut parent = property(text("x"));
        parent
            .sub_attributes_mut()
            .insert(name(SPEED), instances(vec![(None, declared_date_time("2024-01-01T00:00:00.5Z"))]));
        let mut state = entity(vec![(SPEED, instances(vec![(None, parent)]))]);
        state.canonicalize_values(None);
        let held = state.attributes().get(SPEED).and_then(|held| held.get(None)).expect("the parent");
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
        let mut state = entity(vec![(
            SPEED,
            instances(vec![(
                None,
                list(vec![
                    ListObject::Bare(uri("urn:a")),
                    ListObject::Literal(uri("urn:b")),
                    ListObject::Wrapped(uri("urn:c")),
                ]),
            )]),
        )]);
        state.canonicalize_values(None);
        pretty_assertions::assert_eq!(
            state,
            entity(vec![(
                SPEED,
                instances(vec![(
                    None,
                    list(vec![
                        ListObject::Wrapped(uri("urn:a")),
                        ListObject::Wrapped(uri("urn:b")),
                        ListObject::Wrapped(uri("urn:c"))
                    ]),
                )]),
            )])
        );
    }

    #[test]
    fn observed_at_and_expires_at_are_committed_in_their_rendered_spelling() {
        let mut instance = property(text("x"));
        instance.set_observed_at(Some("2024-01-01T00:00:00.50Z".parse().expect("a DateTime")));
        instance.set_expires_at(Some("2030-01-01T00:00:00.000Z".parse().expect("a DateTime")));
        let mut state = entity(vec![(SPEED, instances(vec![(None, instance)]))]);
        state.set_expires_at(Some("2031-01-01T00:00:00,000000Z".parse().expect("a DateTime")));
        state.canonicalize_values(None);
        let held = state.attributes().get(SPEED).and_then(|held| held.get(None)).expect("the instance");
        pretty_assertions::assert_eq!(held.observed_at().map(DateTimeText::as_str), Some("2024-01-01T00:00:00.500Z"));
        pretty_assertions::assert_eq!(held.expires_at().map(DateTimeText::as_str), Some("2030-01-01T00:00:00Z"));
        pretty_assertions::assert_eq!(state.expires_at().map(DateTimeText::as_str), Some("2031-01-01T00:00:00Z"));
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
    fn restating_a_date_time_in_another_spelling_keeps_its_modified_at() {
        let first = SystemInstant::new(DateTime::UNIX_EPOCH);
        let later = SystemInstant::new(DateTime::UNIX_EPOCH + Duration::days(1));
        let mut previous = entity(vec![(SPEED, instances(vec![(None, declared_date_time("2024-01-01T00:00:00.500Z"))]))]);
        previous.settle(None, first);
        let mut restated = entity(vec![(SPEED, instances(vec![(None, declared_date_time("2024-01-01T00:00:00.5Z"))]))]);
        restated.settle(Some(&previous), later);
        pretty_assertions::assert_eq!(restated, previous);
    }
}
