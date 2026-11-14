//! The members of an admitted query that select entities, open to narrowing.

use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::written_entity_selector::WrittenEntitySelector;

/// A mutable view of the identifier list and the clause 5.2.33 selectors of an
/// admitted query.
///
/// An Entity Map fixes the entities a later request considers (clause 5.5.14),
/// so the request is held to the map by rewriting these two members. The view
/// reaches nothing else, so the `q` filter the query was admitted with stays
/// the filter the query carries.
#[derive(Debug)]
pub struct SelectionMut<'a> {
    /// The flat `id` list, or the single selector's summary.
    pub id: &'a mut Option<Vec<Identifier>>,
    /// The clause 5.2.33 selectors of a POST query, empty for a GET.
    pub entity_selectors: &'a mut Vec<WrittenEntitySelector>,
}
