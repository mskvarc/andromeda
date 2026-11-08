//! Compacting the nodes of a structured value (JSON-LD 1.1 API section 6.1,
//! the node-object case).
//!
//! What expansion makes of a JSON object inside a Property value is a node:
//! an optional `@id`, optional `@type` entries and members keyed by expanded
//! IRIs. Its compaction writes the `@id` and the types, then every member as an
//! expanded property, recursively. A `ListRelationship` item in its normalized
//! spelling (clause 4.5.22.2) is such a node too, holding one `hasObject`
//! reference.

use crate::{
    compacted::{Compacted, CompactedObject},
    compacted_mode::{CompactedMode, MemberValues},
    expanded_item::{ExpandedItem, NodeIdentifier, node_identifier},
    reserved_terms::reserved,
    undecidable::Undecidable,
    walk::{Walk, member_term},
};
use andromeda_ngsi_ld_entity::{document::value_carrier::ValueCarrier, expanded_value::ValueNode, value_mode::Stored};
use andromeda_ngsi_ld_vocabulary::ngsi_ld_null::NULL_URI;
use jsonld::Term;

impl Walk<'_> {
    /// Compact a node of a structured value.
    pub(crate) fn compact_value_node<'a, M: CompactedMode>(
        &mut self,
        node: &'a ValueNode<M>,
        id: Option<&NodeIdentifier>,
    ) -> Result<Compacted<'a>, Undecidable> {
        let mut result = CompactedObject::new();
        let types: Vec<_> = node.types().iter().map(|ty| Term::Id(node_identifier(ty.as_str()))).collect();
        let id = id.map(|id| Term::Id(id.clone()));
        self.write_identity(&mut result, id.as_ref(), &types)?;
        for (name, member) in node.members() {
            let property = member_term(name);
            let items = match M::member_values(member) {
                MemberValues::Values(values) => ExpandedItem::of_values(values)?,
                MemberValues::Null => vec![ExpandedItem::text(NULL_URI.as_str())],
            };
            self.compact_property(&mut result, &property, &items)?;
        }
        Ok(Compacted::Object(result))
    }

    /// Compact a `ListRelationship` item written as a Relationship object,
    /// `{"hasObject": [{"@id": uri}]}`.
    pub(crate) fn compact_wrapped_object<'a>(&mut self, uri: &str) -> Result<Compacted<'a>, Undecidable> {
        let mut result = CompactedObject::new();
        let target: [ExpandedItem<'a, Stored>; 1] = [ExpandedItem::reference(uri)];
        self.compact_property(&mut result, reserved().carrier(ValueCarrier::Object), &target)?;
        Ok(Compacted::Object(result))
    }
}
