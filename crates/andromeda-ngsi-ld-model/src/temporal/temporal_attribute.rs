//! The instances one attribute holds in a Temporal Evolution of an Entity
//! (ETSI GS CIM 009 clause 4.5.7).
//!
//! A temporal representation gives each attribute the sequence of instances it
//! took over time, several of them possibly sharing one `datasetId`, each
//! carrying its own `instanceId` and the Temporal Properties that place it. A
//! deletion is one more instance: its family's deletion encoding in the value
//! member beside a `deletedAt` (clause 4.5.0), so it is read and written like
//! any other instance.

use crate::{
    attribute::{
        instance::{Attribute, AttributeValue},
        instance_member::ReadError,
    },
    node_refusal::NodeRefusal,
    temporal::simplified_unfold::instances_of,
};
use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
use andromeda_ngsi_ld_representation::compact_node::{CompactNode, NodeShape};
use andromeda_ngsi_ld_vocabulary::compact::properties as compact_properties;

/// The instances of one attribute of a temporal entity, in the order the
/// representation lists them, and the family they belong to.
///
/// The family is the one of the first instance; it is kept apart so an
/// attribute a clause 6.3.10 page cut left without any instance can still name
/// its `type` and its plural member in the simplified representation (clause
/// 4.5.9).
#[derive(Clone, Debug)]
pub struct TemporalAttribute {
    /// The family the attribute's instances belong to.
    family: AttributeType,
    /// The instances; none only for an attribute the page cut emptied.
    instances: Vec<Attribute>,
}

impl TemporalAttribute {
    /// An attribute whose instances were all left beyond a page cut, of the
    /// family they recorded.
    #[must_use]
    pub const fn emptied(family: AttributeType) -> TemporalAttribute {
        TemporalAttribute { family, instances: Vec::new() }
    }

    /// The attribute holding `instances`, of the family of the first one;
    /// `None` when there is none.
    #[must_use]
    pub fn observed(instances: Vec<Attribute>) -> Option<TemporalAttribute> {
        let family = instances.first()?.attribute_type();
        Some(TemporalAttribute { family, instances })
    }

    /// The family the attribute's instances belong to.
    #[must_use]
    pub const fn family(&self) -> AttributeType {
        self.family
    }

    /// The instances, in order.
    #[must_use]
    pub fn instances(&self) -> &[Attribute] {
        &self.instances
    }

    /// The instances, for a mutation.
    pub const fn instances_mut(&mut self) -> &mut Vec<Attribute> {
        &mut self.instances
    }

    /// Give the instances up.
    #[must_use]
    pub fn into_instances(self) -> Vec<Attribute> {
        self.instances
    }

    /// The instances as the multi-attribute member the compact model writes.
    #[must_use]
    pub fn into_member(self) -> AttributeValue {
        AttributeValue::Multiple(self.instances)
    }

    /// The attribute a compact node written under `name` is: one instance
    /// object, or an array of them, each object in the clause 4.5.9
    /// simplified representation unfolded into the instances its pairs stand
    /// for. An empty array holds no instance and names no family, so it is no
    /// attribute (`None`): a peer's page cut leaves nothing to merge or to
    /// write.
    ///
    /// # Errors
    ///
    /// Returns the refusal of the first element that is neither an instance of
    /// any family nor a simplified attribute object, located at the element,
    /// and the node's [`CompactNode::Unreadable`].
    pub fn from_node<'n, N: CompactNode<'n>>(name: &str, node: N) -> Result<Option<TemporalAttribute>, ReadError<'n, N>> {
        Ok(TemporalAttribute::observed(instances_of(name, node)?))
    }

    /// The reified Scope history (clause 4.5.6) a compact object holds as its
    /// `scope` member, `None` when it holds none. It is the one member read:
    /// the object need name neither an `id` nor a `type`.
    ///
    /// # Errors
    ///
    /// Returns [`NodeRefusal::NotAnObject`] for a node that is no object, and
    /// as [`TemporalAttribute::from_node`], located at `scope`.
    pub fn scope_history_of<'n, N: CompactNode<'n>>(node: N) -> Result<Option<TemporalAttribute>, ReadError<'n, N>> {
        let NodeShape::Object(members) = node.shape() else {
            return Err(NodeRefusal::NotAnObject.into());
        };
        match members.get(compact_properties::SCOPE) {
            Some(history) => TemporalAttribute::from_node(compact_properties::SCOPE, history).map_err(|error| error.within_member(compact_properties::SCOPE)),
            None => Ok(None),
        }
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
        node_refusal::{NodeReadError, NodeRefusal, RefusedNode},
        temporal::temporal_attribute::TemporalAttribute,
    };
    use andromeda_json_literal::piece::JsonPiece;
    use serde_json::{Value, json, value::RawValue};

    /// The Scope history of the object `body`, or the node the read refused.
    fn history_of(body: &Value) -> Result<Option<TemporalAttribute>, RefusedNode> {
        let raw = RawValue::from_string(body.to_string()).expect("raw JSON");
        match TemporalAttribute::scope_history_of(JsonPiece::new(&raw)) {
            Ok(history) => Ok(history),
            Err(NodeReadError::Refused(refused)) => Err(refused),
            Err(NodeReadError::Unreadable(unreadable)) => panic!("a parsed body is readable: {unreadable}"),
        }
    }

    /// Clause 4.5.6: both instances of a reified Scope are read, with their
    /// temporal members, from an object naming no `id` or `type`.
    #[test]
    fn a_reified_scope_is_read_from_an_object_naming_no_entity() {
        let history = history_of(&json!({
            "scope": [
                {"type": "Property", "value": "/Madrid", "createdAt": "2025-01-01T00:00:01Z"},
                {"type": "Property", "value": "urn:ngsi-ld:null", "deletedAt": "2025-01-02T00:00:00Z"}
            ],
            "speed": [{"type": "Property", "value": 1}]
        }))
        .expect("the object reads")
        .expect("it holds a Scope history");
        pretty_assertions::assert_eq!(history.instances().len(), 2);
    }

    #[test]
    fn an_object_without_a_scope_member_holds_no_history() {
        assert!(
            history_of(&json!({"speed": [{"type": "Property", "value": 1}]}))
                .expect("the object reads")
                .is_none()
        );
    }

    #[test]
    fn a_node_that_is_no_object_is_refused() {
        let refused = history_of(&json!([1])).expect_err("an array is no entity");
        assert!(matches!(refused.refusal, NodeRefusal::NotAnObject), "{refused:?}");
    }

    /// An instance no family admits is refused at the `scope` member.
    #[test]
    fn an_instance_of_no_family_is_refused_at_the_scope_member() {
        let refused = history_of(&json!({"scope": [{"type": "Nothing"}]})).expect_err("the instance is refused");
        pretty_assertions::assert_eq!(refused.path.to_string(), "/scope/0", "{refused:?}");
    }
}
