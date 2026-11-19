//! The two clause 6.4.3.2 opt-in lists a subscription's `q` filter reads,
//! expanded through the request `@context` (Table 5.2.12-1 `expandValues` and
//! `jsonKeys`).
//!
//! A subscription's filter is evaluated against notified entity data by a
//! matcher that holds no `@context`, so the coercion `expandValues` asks for
//! happens once, when the filter is written, and what is stored is the filter
//! matching consumes. The lists are stored expanded too, so the stored members
//! name the same IRIs the stored `q` is expanded under.

use crate::{error::SubscriptionError, term_expansion::expand_names};
use andromeda_ngsi_ld_lexicon::{attribute_iri::AttributeIri, attribute_name::AttributeName};
use andromeda_ngsi_ld_model::clearable::Clearable;
use andromeda_ngsi_ld_query::term_expander::TermExpander;

/// Expand both lists a write names. A list the write does not name, and one
/// it clears, opts nothing in.
///
/// # Errors
///
/// Returns [`SubscriptionError::Query`] when a name does not expand to a
/// usable attribute name.
pub(crate) async fn expanded_opt_ins(
    expand_values: &Clearable<Vec<AttributeName>>,
    json_keys: &Clearable<Vec<AttributeName>>,
    expander: &impl TermExpander,
) -> Result<(Clearable<Vec<AttributeIri>>, Clearable<Vec<AttributeIri>>), SubscriptionError> {
    Ok((expand_names(expand_values, expander).await?, expand_names(json_keys, expander).await?))
}
