//! The structure of an entity recorded against the budget of values the entity
//! may still hold.
//!
//! An attribute, an instance and a sub-attribute each cost the entity at least
//! one value when it is counted (ETSI GS CIM 009 v1.9.1 clause 6.3.2), and each
//! costs the broker far more than the text that wrote it. A body of millions of
//! them is therefore read against the budget too: once it is spent the entity
//! is certain to be over its limit, and the rest of that structure is skipped
//! instead of recorded. A node read without a budget records everything.

use andromeda_ngsi_ld_representation::{
    compact_node::CompactNode,
    value_budget::{BudgetMark, Spent, ValueBudget},
};

/// Whether the entity `node` belongs to has no attribute, instance or
/// sub-attribute left to record.
pub(crate) fn is_spent<'n, N: CompactNode<'n>>(node: N) -> bool {
    node.value_budget().is_some_and(ValueBudget::is_structure_spent)
}

/// Record one attribute, instance or sub-attribute of the entity `node`
/// belongs to.
pub(crate) fn spend<'n, N: CompactNode<'n>>(node: N) {
    if let Some(budget) = node.value_budget() {
        budget.spend_structure();
    }
}

/// Note what the entity `node` belongs to may still record, when it is held to
/// a limit.
pub(crate) fn mark<'n, N: CompactNode<'n>>(node: N) -> Option<BudgetMark> {
    node.value_budget().map(ValueBudget::mark)
}

/// What was recorded since `mark`, net of refunds.
pub(crate) fn spent_since<'n, N: CompactNode<'n>>(node: N, mark: Option<BudgetMark>) -> Spent {
    match (node.value_budget(), mark) {
        (Some(budget), Some(mark)) => budget.spent_since(mark),
        (Some(_) | None, None) | (None, Some(_)) => Spent::default(),
    }
}

/// Give back what a member recorded that a later one of the same name
/// replaced.
pub(crate) fn refund<'n, N: CompactNode<'n>>(node: N, spent: Spent) {
    if let Some(budget) = node.value_budget() {
        budget.refund_spent(spent);
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        attribute::{
            instance::{Attribute, AttributeValue},
            structure_budget::{is_spent, mark, refund, spend, spent_since},
        },
        entity::Entity,
        request_body::{DecodeBounds, RequestBody},
    };
    use andromeda_json_literal::piece::JsonPiece;
    use andromeda_ngsi_ld_geometry::geometry_census::GeometryPositionLimit;
    use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
    use andromeda_ngsi_ld_representation::{
        compact_node::context_reading::ContextReading,
        value_budget::{Spent, ValueBudget},
    };
    use serde_json::{Value, json, value::RawValue};
    use std::num::NonZeroUsize;

    fn bounds(entity_values: usize) -> DecodeBounds {
        DecodeBounds::new(
            GeometryPositionLimit::new(NonZeroUsize::new(1_000_000).expect("a positive limit")),
            JsonValueLimit::new(NonZeroUsize::new(entity_values).expect("a positive limit")),
        )
    }

    fn read(text: &str, limit: Option<usize>) -> Entity<AttributeValue> {
        let reading = ContextReading::keyword_spellings();
        match limit {
            Some(limit) => <Entity>::from_bounded_request_text(text.as_bytes(), reading, bounds(limit)),
            None => <Entity>::from_request_text(text.as_bytes(), reading),
        }
        .expect("the entity reads")
    }

    fn property() -> Value {
        json!({"type": "Property", "value": 1})
    }

    fn entity_with(members: impl IntoIterator<Item = (String, Value)>) -> String {
        let mut entity = serde_json::Map::new();
        entity.insert("id".to_owned(), json!("urn:ngsi-ld:T:1"));
        entity.insert("type".to_owned(), json!("T"));
        entity.extend(members);
        Value::Object(entity).to_string()
    }

    fn attributes(count: usize) -> String {
        entity_with((0..count).map(|index| (format!("a{index}"), property())))
    }

    #[test]
    fn attributes_past_the_limit_are_cut_and_the_identity_is_still_read() {
        let entity = read(&attributes(50), Some(10));
        assert!(entity.attrs.len() < 50, "{}", entity.attrs.len());
        assert!(!entity.attrs.is_empty());
    }

    #[test]
    fn attributes_within_the_limit_are_read_whole() {
        pretty_assertions::assert_eq!(read(&attributes(50), Some(1_000)).attrs.len(), 50);
    }

    #[test]
    fn attributes_without_bounds_are_read_whole() {
        pretty_assertions::assert_eq!(read(&attributes(50), None).attrs.len(), 50);
    }

    #[test]
    fn the_instances_of_an_attribute_past_the_limit_are_cut() {
        let text = entity_with([("a".to_owned(), Value::Array(vec![property(); 60]))]);
        let AttributeValue::Multiple(instances) = &read(&text, Some(10)).attrs["a"] else {
            panic!("an array of instances");
        };
        assert!(instances.len() < 60, "{}", instances.len());
    }

    #[test]
    fn the_sub_attributes_of_an_instance_past_the_limit_are_cut() {
        let mut instance = property();
        for index in 0..60 {
            instance[format!("s{index}")] = property();
        }
        let text = entity_with([("a".to_owned(), instance)]);
        let AttributeValue::Single(attribute) = &read(&text, Some(10)).attrs["a"] else {
            panic!("one instance");
        };
        let Attribute::Property(property) = attribute.as_ref() else {
            panic!("a Property");
        };
        assert!(property.common.sub_attributes.len() < 60, "{}", property.common.sub_attributes.len());
    }

    /// An attribute written again replaces the earlier one, so only the last
    /// is what the entity holds and what the budget is spent on.
    #[test]
    fn an_attribute_written_again_gives_back_what_the_replaced_one_was_recorded_for() {
        let text = format!(
            r#"{{"id":"urn:ngsi-ld:T:1","type":"T",{repeated},"z":{{"type":"Property","value":1}}}}"#,
            repeated = vec![format!(r#""a":{}"#, Value::Array(vec![property(); 10])); 4].join(",")
        );
        let entity = read(&text, Some(30));
        assert!(entity.attrs.contains_key("z"));
        pretty_assertions::assert_eq!(entity.attrs.len(), 2);
    }

    #[test]
    fn a_recorded_element_spends_the_budget_and_a_refund_gives_it_back() {
        let raw = RawValue::from_string("{}".to_owned()).expect("JSON");
        let budget = ValueBudget::new(JsonValueLimit::new(NonZeroUsize::MIN));
        let node = andromeda_ngsi_ld_representation::compact_node::bounded_piece::BoundedPiece::new(
            JsonPiece::new(&raw),
            GeometryPositionLimit::new(NonZeroUsize::MIN),
            &budget,
        );
        let before = mark(node);
        spend(node);
        spend(node);
        assert!(is_spent(node));
        refund(node, spent_since(node, before));
        assert!(!is_spent(node));
    }

    #[test]
    fn a_node_without_a_budget_is_never_spent() {
        let raw = RawValue::from_string("{}".to_owned()).expect("JSON");
        let node = JsonPiece::new(&raw);
        spend(node);
        assert!(!is_spent(node));
        pretty_assertions::assert_eq!(spent_since(node, mark(node)), Spent::default());
    }
}
