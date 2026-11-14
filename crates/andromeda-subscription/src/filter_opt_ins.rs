//! The two clause 6.4.3.2 opt-in lists a subscription's `q` filter reads,
//! expanded through the request `@context` (Table 5.2.12-1 `expandValues` and
//! `jsonKeys`).
//!
//! A subscription's filter is evaluated against notified entity data by a
//! matcher that holds no `@context`, so the coercion `expandValues` asks for
//! happens once, when the filter is written, and what is stored is the filter
//! matching consumes. The lists are stored expanded too, so the stored members
//! name the same IRIs the stored `q` is expanded under.

use crate::error::SubscriptionError;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_model::clearable::Clearable;
use andromeda_ngsi_ld_query::{
    filter::{compilation::FilterOptIns, expansion::expand_attribute_names},
    term_expander::TermExpander,
};

/// Expand both lists a write names in place, and the opt-ins the `q` is
/// compiled under. A list the write does not name, and one it clears, opts
/// nothing in.
///
/// # Errors
///
/// Returns [`SubscriptionError::Query`] when a name does not expand to a
/// usable attribute name.
pub(crate) async fn expanded_opt_ins(
    expand_values: &mut Clearable<Vec<AttributeName>>,
    json_keys: &mut Clearable<Vec<AttributeName>>,
    expander: &impl TermExpander,
) -> Result<FilterOptIns, SubscriptionError> {
    expand_list(expand_values, expander).await?;
    expand_list(json_keys, expander).await?;
    Ok(FilterOptIns {
        // clone: the write keeps its lists while the filter expansion reads their names
        json_keys: json_keys.as_set().into_iter().flatten().cloned().collect(),
        expand_values: expand_values.as_set().into_iter().flatten().cloned().collect(),
    })
}

/// Expand the names of one opt-in list the write names.
async fn expand_list(list: &mut Clearable<Vec<AttributeName>>, expander: &impl TermExpander) -> Result<(), SubscriptionError> {
    if let Some(names) = list.as_set_mut() {
        *names = expand_attribute_names(names, expander).await?;
    }
    Ok(())
}
