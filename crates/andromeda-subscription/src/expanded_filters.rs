//! The `q` and `csf` of a subscription in the expanded stage.
//!
//! A request carries the filters as its client wrote them, in the written
//! stage, and a subscription is matched and stored by their expanded forms
//! (clause 5.5.7). Expansion leaves the request as written and makes
//! [`ExpandedFilters`]; a stored subscription holds the text of its expanded
//! filters, which [`StoredFilters`] reads as the stage they are in.

use crate::error::SubscriptionError;
#[cfg(test)]
use andromeda_ngsi_ld_model::subscription_request::SubscriptionRequest;
use andromeda_ngsi_ld_model::{clearable::Clearable, subscription::SubscriptionFilters};
use andromeda_ngsi_ld_query::{filter::Filter, name_stage::Expanded, query_error::QueryError};
#[cfg(test)]
use andromeda_ngsi_ld_query::{name_stage::Written, namespace_expander::EXAMPLE_NAMESPACE};

/// The expanded filters of one subscription request, each as the request
/// writes its member: absent, deleted, or set.
#[derive(Debug, Default)]
pub struct ExpandedFilters {
    /// The request's `q`, expanded under its opt-in lists.
    pub q: Clearable<Filter<Expanded>>,
    /// The request's `csf`, its properties expanded.
    pub csf: Clearable<Filter<Expanded>>,
}

#[cfg(test)]
impl ExpandedFilters {
    /// The filters `request` writes, expanded under the example namespace.
    pub(crate) fn under_example_namespace(request: &SubscriptionRequest) -> ExpandedFilters {
        let expand = |written: &Clearable<Filter<Written>>| match written {
            Clearable::Set(filter) => Clearable::Set(EXAMPLE_NAMESPACE.expanded(filter.clone()).expect("the names expand")),
            Clearable::Absent => Clearable::Absent,
            Clearable::NgsiLdNull => Clearable::NgsiLdNull,
        };
        ExpandedFilters {
            q: expand(&request.q),
            csf: expand(&request.csf),
        }
    }
}

/// The expanded filters a subscription is stored with.
#[derive(Debug, Default)]
pub struct StoredFilters {
    /// The stored `q`, `None` when the subscription has none.
    pub q: Option<Filter<Expanded>>,
    /// The stored `csf`, `None` when the subscription has none.
    pub csf: Option<Filter<Expanded>>,
}

impl StoredFilters {
    /// The filters of a stored subscription in the expanded stage.
    ///
    /// # Errors
    ///
    /// Returns [`SubscriptionError::Query`] when a stored filter names a term
    /// where an absolute IRI has to stand.
    pub fn of(filters: &SubscriptionFilters) -> Result<StoredFilters, SubscriptionError> {
        // clone: the checked filter is an owned tree, and the subscription keeps the text it was stored with
        let expanded = |filter: &Option<Filter<_>>| filter.clone().map(Filter::into_expanded).transpose().map_err(QueryError::from);
        Ok(StoredFilters {
            q: expanded(&filters.q)?,
            csf: expanded(&filters.csf)?,
        })
    }
}
