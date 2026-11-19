//! The names of a notification request after expansion through its `@context`.

use andromeda_ngsi_ld_lexicon::attribute_iri::AttributeIri;
use andromeda_ngsi_ld_query::projection::expanded_projection_list::ExpandedProjectionList;

/// The members of a notification request that hold attribute names, in the expanded stage a subscription stores them in (clause 5.5.7).
#[derive(Debug, Clone, Default)]
pub struct ExpandedNotificationTerms {
    /// The notified attributes (clause 5.2.14 `attributes`).
    pub attributes: Option<Vec<AttributeIri>>,
    /// The `pick` lists, one per projection string the client wrote.
    pub pick: Option<Vec<ExpandedProjectionList>>,
    /// The `omit` lists, one per projection string the client wrote.
    pub omit: Option<Vec<ExpandedProjectionList>>,
}
