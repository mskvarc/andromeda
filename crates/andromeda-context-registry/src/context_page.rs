//! One page of the List @contexts collection (clause 5.13.3).

use crate::listed_contexts::ListedContexts;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;

/// The Contexts one page window selected, in local-identifier order.
#[derive(Debug)]
pub struct ContextPage {
    /// The page, projected to the requested detail level.
    pub contexts: ListedContexts,
    /// The local identifiers of the first and last Context on the page, from
    /// which the neighbouring cursors are built; absent on an empty page.
    ///
    /// A summary entry is written as its public URL, so the identifiers the
    /// page is keyed by are carried beside the entries rather than read back
    /// from them.
    pub boundary: Option<(Identifier, Identifier)>,
}
