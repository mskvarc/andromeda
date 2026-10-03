//! A Context Source's temporal answer, narrowed to the members a split query's
//! filters read and laid out as the Temporal Evolution a write would carry.
//!
//! Clause 5.7.4.4 judges an entity held in several parts on the union of its
//! parts, so the part a source answered with is read for exactly the leaves of
//! the query's value, geo and scope filters. A member none of them reads is not
//! read at all: whatever it holds cannot change the outcome, and a member the
//! write expansion would refuse cannot then keep the readable ones from being
//! judged. A `temporalValues` attribute (clause 4.5.9) holds its instances as
//! `[value, instant]` pairs, which are rewritten as the instances they stand for
//! so the plural member unfolds as a written one does.

use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, time_prop::TimeProp};
use andromeda_ngsi_ld_query::{entity_selection::EntitySelection, filter::referenced_attributes::referenced_attributes, term_expander::TermExpander};
use andromeda_ngsi_ld_vocabulary::compact::properties as compact_properties;
use serde_json::{Map, Value};
use std::{
    collections::{HashMap, HashSet},
    mem::take,
};

/// The members of a temporal representation every narrowing keeps: what
/// names the entity.
const IDENTIFYING_MEMBERS: [&str; 2] = ["id", "type"];

/// Narrows the answers of one split query to the members its filters read.
///
/// The decision for a member key is memoised, so a key every answered entity
/// carries is expanded once per query rather than once per entity.
pub(crate) struct AnswerNarrowing<'q, X: TermExpander> {
    /// The expander the answers' compact keys are read under: the request's
    /// own `@context`, which the answers were compacted against.
    expander: &'q X,
    /// The expanded names of the attributes the filters read.
    read: HashSet<&'q str>,
    /// Whether a scope requirement reads the entity's Scope (clause 4.19).
    reads_scope: bool,
    /// The Temporal Property a `temporalValues` pair positions its value on.
    timeproperty: TimeProp,
    /// Whether each compact member key met so far names an attribute read.
    decided: HashMap<String, bool>,
}

impl<'q, X: TermExpander> AnswerNarrowing<'q, X> {
    /// The narrowing to the members `selection`'s filters read, compact keys
    /// read under `expander`.
    pub(crate) fn new(selection: &'q EntitySelection, timeproperty: TimeProp, expander: &'q X) -> AnswerNarrowing<'q, X> {
        let mut read: HashSet<&'q str> = selection
            .q_expr
            .as_ref()
            .map(|expr| referenced_attributes(expr).into_iter().map(AttributeName::as_str).collect())
            .unwrap_or_default();
        read.extend(selection.geo.as_ref().map(|geo| geo.geoproperty.as_str()));
        AnswerNarrowing {
            expander,
            read,
            reads_scope: selection.scope_q.is_some(),
            timeproperty,
            decided: HashMap::new(),
        }
    }

    /// `answer` reduced to the members the filters read, its `temporalValues`
    /// pairs rewritten as instances; an answer that is no entity object is
    /// left for the expansion to refuse.
    pub(crate) async fn narrow(&mut self, answer: &Value) -> Value {
        let Some(members) = answer.as_object() else {
            return answer.clone(); // clone: the answer stays as the merged response carries it
        };
        let mut kept = Map::with_capacity(members.len());
        for (key, value) in members {
            let keep = if IDENTIFYING_MEMBERS.contains(&key.as_str()) {
                true
            } else if key == compact_properties::SCOPE {
                self.reads_scope
            } else {
                self.reads(key).await
            };
            if keep {
                // clone: the answer stays as the merged response carries it
                kept.insert(key.clone(), instances_of(value.clone(), self.timeproperty));
            }
        }
        Value::Object(kept)
    }

    /// Whether the compact member `key` names an attribute the filters read.
    ///
    /// A key that does not expand names no attribute the filters could have
    /// named, so it is not read.
    async fn reads(&mut self, key: &str) -> bool {
        if let Some(decided) = self.decided.get(key) {
            return *decided;
        }
        let reads = match self.expander.expand_term(key).await {
            Ok(expanded) => self.read.contains(expanded.as_str()),
            Err(_) => false,
        };
        self.decided.insert(key.to_owned(), reads);
        reads
    }
}

/// The member `value` with each `temporalValues` attribute object's pairs
/// rewritten as instances: `[value, instant]` becomes `{<value member>:
/// value}`, the instant kept as `observedAt` when that is the Temporal
/// Property the pairs are positioned on.
fn instances_of(value: Value, timeproperty: TimeProp) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.into_iter().map(|item| pairs_as_instances(item, timeproperty)).collect()),
        object @ Value::Object(_) => pairs_as_instances(object, timeproperty),
        other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)) => other,
    }
}

/// One attribute object with the pairs of its family's plural member, if it
/// holds them, rewritten as instances.
fn pairs_as_instances(item: Value, timeproperty: TimeProp) -> Value {
    let Value::Object(mut attribute) = item else {
        return item;
    };
    let Some(family) = attribute
        .get("type")
        .and_then(Value::as_str)
        .and_then(|declared| declared.parse::<AttributeType>().ok())
    else {
        return Value::Object(attribute);
    };
    if let Some(Value::Array(pairs)) = attribute.get_mut(family.temporal_values_key()) {
        for pair in pairs.iter_mut() {
            pair_as_instance(pair, family, timeproperty);
        }
    }
    Value::Object(attribute)
}

/// Rewrite one clause 4.5.9 `[value, instant]` pair as the instance it stands
/// for; anything else is left as it is.
fn pair_as_instance(pair: &mut Value, family: AttributeType, timeproperty: TimeProp) {
    let Value::Array(elements) = pair else {
        return;
    };
    let [value, instant] = match <[Value; 2]>::try_from(take(elements)) {
        Ok(paired) => paired,
        Err(unpaired) => {
            *elements = unpaired;
            return;
        }
    };
    let mut instance = Map::from_iter([(family.compact_value_key().to_owned(), value)]);
    match timeproperty {
        TimeProp::Observed => {
            instance.insert(<&'static str>::from(timeproperty).to_owned(), instant);
        }
        TimeProp::Created | TimeProp::Modified | TimeProp::Deleted => {}
    }
    *pair = Value::Object(instance);
}

#[cfg(test)]
mod tests {
    use crate::answer_narrowing::{instances_of, pairs_as_instances};
    use andromeda_ngsi_ld_lexicon::time_prop::TimeProp;
    use serde_json::json;

    /// Clause 4.5.9: a `temporalValues` pair is the value and the instant it
    /// was observed at.
    #[test]
    fn a_pair_becomes_the_instance_it_stands_for() {
        let attribute = json!({"type": "Property", "values": [[60, "2020-08-01T12:00:00Z"], [70, "2020-08-01T13:00:00Z"]]});
        pretty_assertions::assert_eq!(
            pairs_as_instances(attribute, TimeProp::Observed),
            json!({"type": "Property", "values": [
                {"value": 60, "observedAt": "2020-08-01T12:00:00Z"},
                {"value": 70, "observedAt": "2020-08-01T13:00:00Z"}
            ]})
        );
    }

    /// Every family's plural member holds its pairs under its own value member.
    #[test]
    fn a_relationship_pair_names_its_object() {
        let attribute = json!([{"type": "Relationship", "datasetId": "urn:ds:1", "objects": [["urn:a", "2020-08-01T12:00:00Z"]]}]);
        pretty_assertions::assert_eq!(
            instances_of(attribute, TimeProp::Modified),
            json!([{"type": "Relationship", "datasetId": "urn:ds:1", "objects": [{"object": "urn:a"}]}])
        );
    }

    /// A normalized instance is no pair and stays as it is.
    #[test]
    fn a_normalized_instance_is_left_alone() {
        let attribute = json!([{"type": "Property", "value": 60, "observedAt": "2020-08-01T12:00:00Z"}]);
        pretty_assertions::assert_eq!(instances_of(attribute.clone(), TimeProp::Observed), attribute);
    }
}
