//! The term walk: compacting the model one term at a time.
//!
//! The walk follows the JSON-LD 1.1 API compaction algorithm (section 6.1)
//! over the expanded document the model stands for, without that document
//! ever being built: each node is written straight into the compacted object,
//! each key and value decided by the processed context (see
//! [`TermContext`](crate::term_context::TermContext)). This module holds the
//! walk's state and its dispatch over the items of an expanded property; the
//! steps live with the nodes they compact.

use crate::{
    compacted::{Compacted, CompactedObject},
    compacted_mode::CompactedMode,
    expanded_item::{ExpandedItem, node_identifier},
    member_insertion::add_value,
    term_context::TermContext,
    undecidable::Undecidable,
};
use andromeda_ngsi_ld_entity::expanded_value_name::ExpandedValueName;
use jsonld::{BlankIdBuf, IriBuf, Term, syntax::Keyword};
use std::sync::Arc;

/// How a geometry is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GeometryOutput {
    /// As the compacted `GeoJSON` object, every position written out.
    Written,
    /// As the typed geometry it is, wherever the compact model reads the
    /// compacted object as that geometry, so no position is written only to
    /// be read back.
    Typed,
}

/// One run of the walk over one document.
#[derive(Debug)]
pub(crate) struct Walk<'t> {
    /// The processed context every decision is taken in.
    pub(crate) terms: &'t TermContext,
    /// How geometries are written.
    pub(crate) geometries: GeometryOutput,
}

impl<'t> Walk<'t> {
    /// A walk writing geometries as `geometries` says.
    pub(crate) const fn new(terms: &'t TermContext, geometries: GeometryOutput) -> Walk<'t> {
        Walk { terms, geometries }
    }

    /// Compact one item that is not a list written under its own key: a
    /// value object or node reference by value compaction (JSON-LD 1.1 API
    /// section 6.3), a node by node compaction, a list inside a list as a
    /// nested list object.
    pub(crate) fn compact_item<'a, M: CompactedMode>(&mut self, item: &ExpandedItem<'a, M>, key: &str) -> Result<Compacted<'a>, Undecidable> {
        match item {
            ExpandedItem::Value(value) => self.terms.value(value, key),
            ExpandedItem::Reference(id) => self.terms.node_reference(id, key),
            ExpandedItem::Node(node, id) => self.compact_value_node(*node, id.as_ref()),
            ExpandedItem::List(items) => self.compact_nested_list(items, key),
            ExpandedItem::StoredInstance(stored) => self.compact_stored_instance(stored),
            ExpandedItem::WrittenInstance(written) => self.compact_written_instance(written),
            ExpandedItem::Geometry(value) => self.compact_geometry(value),
            ExpandedItem::WrappedObject(uri) => self.compact_wrapped_object(uri.as_str()),
        }
    }

    /// Write a node's `@id` and `@type` entries (JSON-LD 1.1 API section 6.1,
    /// the `@id` and `@type` steps).
    ///
    /// A compacted type that is a term carrying a type-scoped context is
    /// refused before anything is written, as the algorithm applies such a
    /// context before it compacts the node's members.
    pub(crate) fn write_identity(
        &self,
        result: &mut CompactedObject<'_>,
        id: Option<&Term<IriBuf, BlankIdBuf>>,
        types: &[Term<IriBuf, BlankIdBuf>],
    ) -> Result<(), Undecidable> {
        let compacted_types = types.iter().map(|ty| self.terms.type_term(ty)).collect::<Result<Vec<_>, _>>()?;
        if let Some(id) = id {
            let compacted = self.terms.identifier(id)?;
            result.insert(self.terms.keyword(Keyword::Id), optional_string(compacted.as_ref()));
        }
        let value = match compacted_types.as_slice() {
            [] => return Ok(()),
            [single] => optional_string(single.as_ref()),
            several => Compacted::Array(several.iter().map(|ty| optional_string(ty.as_ref())).collect()),
        };
        let alias = self.terms.keyword(Keyword::Type);
        add_value(result, alias, value, self.terms.types_as_array(alias));
        Ok(())
    }
}

/// The term for an expanded member name, or `None` for a name that is neither
/// an IRI nor a blank node identifier, whose member expansion drops.
pub(crate) fn property_term(name: &str) -> Option<Term<IriBuf, BlankIdBuf>> {
    let id = node_identifier(name);
    id.is_valid().then_some(Term::Id(id))
}

/// The term for the name of a member of a structured value or a foreign
/// member of a geometry. Expansion keeps every such name, a well-formed IRI or
/// not (JSON-LD 1.1 expansion algorithm step 13.3), so each is compacted back
/// into the key it was written as, `.../opening hours` into `opening hours`.
pub(crate) fn member_term(name: &ExpandedValueName) -> Term<IriBuf, BlankIdBuf> {
    Term::Id(node_identifier(name.as_str()))
}

/// A compacted IRI as a JSON string, or `null` where IRI compaction gives
/// nothing.
fn optional_string<'a>(text: Option<&Arc<str>>) -> Compacted<'a> {
    text.map_or(Compacted::Null, |text| Compacted::String(Box::from(&**text)))
}
