//! The names of a subscription request in the expanded stage.
//!
//! A request carries its terms as its client wrote them, in the written stage, and a subscription is matched and stored by their expanded forms (clause 5.5.7). Expansion leaves the request as written and makes [`ExpandedTerms`], each member as the request writes it: absent, deleted, or set.

#[cfg(test)]
use crate::term_expansion::expand_request_terms;
use andromeda_ngsi_ld_lexicon::attribute_iri::AttributeIri;
#[cfg(test)]
use andromeda_ngsi_ld_model::subscription_request::SubscriptionRequest;
use andromeda_ngsi_ld_model::{clearable::Clearable, entity_selector::EntitySelector, expanded_notification_terms::ExpandedNotificationTerms};
use andromeda_ngsi_ld_query::{
    filter::{Filter, compilation::FilterOptIns},
    geo::geo_constraint::GeoConstraint,
    name_stage::Expanded,
};
#[cfg(test)]
use andromeda_ngsi_ld_query::{name_stage::Written, namespace_expander::EXAMPLE_NAMESPACE};
#[cfg(test)]
use futures::executor::block_on;

/// The expanded terms of one subscription request.
#[derive(Debug, Default)]
pub struct ExpandedTerms {
    /// The entity selectors, their types expanded.
    pub entities: Clearable<Vec<EntitySelector<Expanded>>>,
    /// The watched attributes.
    pub watched_attributes: Clearable<Vec<AttributeIri>>,
    /// The attributes, `pick` and `omit` of the request's notification.
    pub notification: ExpandedNotificationTerms,
    /// The `q` filter.
    pub q: Clearable<Filter<Expanded>>,
    /// The `geoQ` geo-query, its geoproperty expanded.
    pub geo_q: Clearable<GeoConstraint<Expanded>>,
    /// The context source filter.
    pub csf: Clearable<Filter<Expanded>>,
    /// The `expandValues` attributes.
    pub expand_values: Clearable<Vec<AttributeIri>>,
    /// The `jsonKeys` attributes.
    pub json_keys: Clearable<Vec<AttributeIri>>,
}

impl ExpandedTerms {
    /// The opt-in lists a `q` filter is compiled under: a list the request does not name, or clears, opts nothing in.
    #[must_use]
    pub fn filter_opt_ins(&self) -> FilterOptIns {
        // clone: the opt-ins are owned by the compilation of the filter, apart from the terms stored
        let listed = |list: &Clearable<Vec<AttributeIri>>| list.as_set().into_iter().flatten().cloned().collect::<Vec<_>>();
        FilterOptIns {
            expand_values: listed(&self.expand_values).into_iter().collect(),
            json_keys: listed(&self.json_keys).into_iter().collect(),
        }
    }
}

#[cfg(test)]
impl ExpandedTerms {
    /// The terms of `request` expanded under the example namespace, its `q` and `csf` without the opt-in coercion.
    pub(crate) fn under_example_namespace(request: &SubscriptionRequest) -> ExpandedTerms {
        let expand = |written: &Clearable<Filter<Written>>| match written {
            Clearable::Set(filter) => Clearable::Set(EXAMPLE_NAMESPACE.expanded(filter.clone()).expect("the names expand")),
            Clearable::Absent => Clearable::Absent,
            Clearable::NgsiLdNull => Clearable::NgsiLdNull,
        };
        let without_filters = SubscriptionRequest {
            q: Clearable::Absent,
            csf: Clearable::Absent,
            ..request.clone()
        };
        let mut terms = block_on(expand_request_terms(&without_filters, &EXAMPLE_NAMESPACE, None)).expect("the terms expand");
        terms.q = expand(&request.q);
        terms.csf = expand(&request.csf);
        terms
    }
}
