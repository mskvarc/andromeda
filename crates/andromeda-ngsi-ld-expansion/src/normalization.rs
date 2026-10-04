//! Which concise-to-normalized rewrite an entity goes through before
//! expansion.
//!
//! A request or a notification may carry attributes in the concise
//! representation (clause 4.5.2.3 and siblings), which expansion reads only
//! after they are rewritten into the normalized one. The rewrite is the
//! representation crate's; what the entity is decides which of its two entity
//! forms applies, because a
//! merge (clause 5.5.12) and a notification (clause 5.8.6) keep a first-level
//! NGSI-LD Null member as the deletion of the whole member rather than reading
//! it as a concise Property value.

use andromeda_ngsi_ld_representation::normalization::{normalize_entity_attributes, normalize_keeping_member_deletions};
use serde_json::{Map, Value};

/// The rewrite an entity fragment goes through before expansion.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum Normalization {
    /// Every attribute member is rewritten into its normalized form (create,
    /// replace, append, update, upsert).
    #[default]
    Entity,
    /// As [`Normalization::Entity`], except that a first-level NGSI-LD Null
    /// member stays as written, the deletion of the whole member: a merge
    /// removes the member by it (clause 5.5.12), and a notification reports
    /// the member deleted by it (clause 5.8.6).
    MemberDeletions,
}

impl Normalization {
    /// Rewrite an entity fragment in place.
    pub fn apply(self, fragment: &mut Value) {
        match self {
            Normalization::Entity => normalize_entity_attributes(fragment),
            Normalization::MemberDeletions => normalize_keeping_member_deletions(fragment),
        }
    }

    /// The normalized form of one attribute member written as `value` under
    /// `name`, as rewriting the whole fragment would leave it.
    #[must_use]
    pub fn apply_to_member(self, name: &str, value: Value) -> Value {
        let mut fragment = Value::Object(Map::from_iter([(name.to_owned(), value)]));
        self.apply(&mut fragment);
        match fragment {
            Value::Object(mut members) => members.remove(name).unwrap_or(Value::Null),
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Array(_) => Value::Null,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::normalization::Normalization;
    use serde_json::json;

    #[test]
    fn a_concise_member_is_rewritten_as_the_whole_fragment_would_be() {
        pretty_assertions::assert_eq!(
            Normalization::Entity.apply_to_member("speed", json!(42)),
            json!({"type": "Property", "value": 42})
        );
    }

    /// Clause 5.5.12: a merge keeps a first-level NGSI-LD Null member as
    /// written, where any other write reads it as a concise Property value.
    #[test]
    fn a_merge_keeps_a_first_level_null_member_as_written() {
        pretty_assertions::assert_eq!(
            Normalization::MemberDeletions.apply_to_member("speed", json!("urn:ngsi-ld:null")),
            json!("urn:ngsi-ld:null")
        );
        pretty_assertions::assert_eq!(
            Normalization::Entity.apply_to_member("speed", json!("urn:ngsi-ld:null")),
            json!({"type": "Property", "value": "urn:ngsi-ld:null"})
        );
    }
}
