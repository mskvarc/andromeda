//! Expansion of the clause 4.21 projection lists of a subscription's
//! notification (Table 5.2.14.1-1 `pick` and `omit`, ETSI GS CIM 009 clause
//! 5.5.7).

use crate::error::SubscriptionError;
use andromeda_ngsi_ld_query::{
    projection::{
        expansion::expand_projection_terms,
        grammar::{ProjectionList, RawProjectionTerm},
    },
    term_expander::TermExpander,
};

/// Replace each projection list with the term tree it expands to.
///
/// Every attribute name expands, at every nesting level, and the clause 4.21
/// keywords stay as written; the tree is what the subscription stores, since
/// an expanded name may hold the separators of the clause 4.21 text.
///
/// # Errors
///
/// Returns [`SubscriptionError::Query`] when a name does not expand to a usable
/// attribute name.
pub(crate) async fn expand_projection_lists(lists: &mut [ProjectionList], expander: &impl TermExpander) -> Result<(), SubscriptionError> {
    for list in lists {
        let terms = expand_projection_terms(list, expander).await?;
        *list = terms.iter().map(RawProjectionTerm::try_from).collect::<Result<Vec<_>, _>>()?.into();
    }
    Ok(())
}
