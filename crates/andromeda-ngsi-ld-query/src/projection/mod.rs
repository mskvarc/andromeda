//! Attribute selection on the response: the `attrs`, `pick` and `omit`
//! parameters (clause 4.21), and the `drop` / `keep` purge modes of the
//! delete-by-query operation (clause 5.6.21).
//!
//! A projection holds its names in two stages, each as a type of its own: the
//! [`WrittenProjectionList`](crate::projection::written_projection_list::WrittenProjectionList)
//! and its terms read the clause 4.21 text as the client wrote it, and the
//! [`ExpandedProjectionList`](crate::projection::expanded_projection_list::ExpandedProjectionList)
//! and its terms hold every attribute name as the IRI the request `@context`
//! expands it to, with the clause 4.21 keywords kept as written. An expanded
//! term tree is what a stored Subscription holds for `pick` and `omit`, because
//! an expanded name may contain the separators of the clause 4.21 text, so the
//! stored form is the tree and not its text.

pub mod attribute_iri_index;
pub mod attrs_selection;
pub mod exclusivity;
pub mod expanded_projection;
pub mod expanded_projection_list;
pub mod expanded_projection_term;
pub mod expansion;
pub mod matching;
mod parser;
pub mod projected_members;
pub mod projection_error;
pub mod purge;
pub mod returned_attributes;
pub mod stored_projection_term;
pub mod written_projection_list;
pub mod written_projection_term;
