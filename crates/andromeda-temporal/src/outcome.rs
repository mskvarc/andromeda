//! What a temporal reader answers stage one of a temporal query with.

use andromeda_ngsi_ld_lexicon::identifier::Identifier;

/// The entities one page of a temporal query selects.
///
/// The reader applies every predicate and the window itself, so the page is
/// final: policy renders it as it stands.
#[derive(Debug)]
pub struct TemporalEntityIdOutcome {
    /// The page's entity identifiers, in the order the page presents them.
    pub entity_ids: Vec<Identifier>,
    /// Every matching entity, when the query asked for the count (clause
    /// 6.3.13).
    pub total: Option<i64>,
}
