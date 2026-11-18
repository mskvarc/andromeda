//! Attribute selection on the response: the `attrs`, `pick` and `omit`
//! parameters (clause 4.21), and the `drop` / `keep` purge modes of the
//! delete-by-query operation (clause 5.6.21).

pub mod attribute_iri_index;
pub mod attrs_selection;
pub mod exclusivity;
pub mod expanded_projection;
pub mod expanded_projection_term;
pub mod expansion;
pub mod matching;
pub mod projected_members;
pub mod purge;
pub mod returned_attributes;
pub mod written_projection_list;
pub mod written_projection_term;
