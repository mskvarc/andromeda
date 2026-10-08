//! The two clause 6.4.3.2 opt-in lists a subscription's `q` filter reads,
//! expanded through the request `@context` (Table 5.2.12-1 `expandValues` and
//! `jsonKeys`).
//!
//! They travel together because both key the same `q` filter and neither means
//! anything without it.

use crate::error::SubscriptionError;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_model::clearable::Clearable;
use andromeda_ngsi_ld_query::{
    filter::{
        expansion::{apply_expand_values, expand_attribute_names, expand_q_expr},
        expression::Expr,
        json_key_attributes::JsonKeyAttributes,
    },
    term_expander::TermExpander,
};
use std::collections::HashSet;

/// The opt-in lists a write names, already expanded.
#[derive(Debug, Default)]
pub(crate) struct FilterOptIns {
    /// Attributes whose `q` values are expanded by JSON-LD type coercion.
    expand_values: HashSet<AttributeName>,
    /// Attributes whose values are uninterpretable as JSON-LD, so the object
    /// keys the filter addresses under them stay literal.
    json_keys: JsonKeyAttributes,
}

impl FilterOptIns {
    /// Expand both lists in place, so the persisted members name the same IRIs
    /// the persisted `q` is expanded into.
    ///
    /// A list the write does not name, and one it clears, opts nothing in.
    ///
    /// # Errors
    ///
    /// Returns [`SubscriptionError::Query`] when a name does not expand to a
    /// usable attribute name.
    pub(crate) async fn expanded(
        expand_values: &mut Clearable<Vec<AttributeName>>,
        json_keys: &mut Clearable<Vec<AttributeName>>,
        expander: &impl TermExpander,
    ) -> Result<FilterOptIns, SubscriptionError> {
        expand_list(expand_values, expander).await?;
        expand_list(json_keys, expander).await?;
        Ok(FilterOptIns {
            // clone: the write keeps its lists while the filter expansion reads their names
            expand_values: expand_values.as_set().into_iter().flatten().cloned().collect(),
            json_keys: json_keys.as_set().into_iter().flatten().cloned().collect(), // clone: as above
        })
    }

    /// Expand a `q` filter's attribute terms under the `jsonKeys` declaration,
    /// then let `expandValues` rewrite the operands it opted in.
    ///
    /// The subscription's filter is evaluated against notified entity data by a
    /// synchronous matcher that has no `@context` and no expander, so the
    /// coercion Table 5.2.12-1 asks for happens once, here, at create and update
    /// time; what is stored is the filter matching consumes. `jsonKeys` is not
    /// applied as a rewrite at all -- it decides how the filter's own terms
    /// expand, so it is handed to the expansion instead.
    ///
    /// # Errors
    ///
    /// Returns [`SubscriptionError::Query`] when a term or an opted-in operand
    /// does not expand.
    pub(crate) async fn expand_q(&self, q: Expr, expander: &impl TermExpander) -> Result<Expr, SubscriptionError> {
        let filter = expand_q_expr(q, &self.json_keys, expander).await?;
        Ok(apply_expand_values(filter, &self.expand_values, expander).await?)
    }
}

/// Expand the names of one opt-in list the write names.
async fn expand_list(list: &mut Clearable<Vec<AttributeName>>, expander: &impl TermExpander) -> Result<(), SubscriptionError> {
    if let Some(names) = list.as_set_mut() {
        *names = expand_attribute_names(names, expander).await?;
    }
    Ok(())
}
