//! The names of a stored subscription in the written stage.

use crate::{entity_selector::EntitySelector, subscription::Subscription};
use andromeda_ngsi_ld_lexicon::{attribute_iri::AttributeIri, attribute_name::AttributeName, reference::NgsiLdReferenceError};
use andromeda_ngsi_ld_query::{
    filter::Filter,
    geo::geo_constraint::GeoConstraint,
    name_stage::Written,
    projection::{
        expanded_projection_list::ExpandedProjectionList,
        projection_error::ProjectionError,
        written_projection_list::WrittenProjectionList,
        written_projection_term::WrittenProjectionTerm,
    },
    type_selection::type_selection_error::TypeSelectionError,
};
use thiserror::Error;

/// Why a stored name has no written spelling.
#[derive(Debug, Error)]
pub enum WrittenTermsError {
    /// An entity selector's type is past the identifier bound a written name carries.
    #[error(transparent)]
    EntityType(#[from] TypeSelectionError),
    /// A `q` or `csf` guard type is past the identifier bound a written name carries.
    #[error(transparent)]
    Filter(#[from] NgsiLdReferenceError),
    /// A projection term is no written term.
    #[error(transparent)]
    Projection(#[from] ProjectionError),
}

/// The members of a stored subscription that hold names, spelled in the written stage: each name is the absolute IRI the stored subscription holds, until the reader's `@context` compacts it.
#[derive(Debug, Clone, Default)]
pub struct WrittenSubscriptionTerms {
    /// The entity selectors (clause 5.2.12 `entities`).
    pub entities: Option<Vec<EntitySelector<Written>>>,
    /// The watched attributes.
    pub watched_attributes: Option<Vec<AttributeName>>,
    /// The notified attributes (clause 5.2.14 `attributes`).
    pub attributes: Option<Vec<AttributeName>>,
    /// The `pick` lists of the notification.
    pub pick: Option<Vec<WrittenProjectionList>>,
    /// The `omit` lists of the notification.
    pub omit: Option<Vec<WrittenProjectionList>>,
    /// The `q` filter.
    pub q: Option<Filter<Written>>,
    /// The `geoQ` geo-query.
    pub geo_q: Option<GeoConstraint<Written>>,
    /// The context source filter.
    pub csf: Option<Filter<Written>>,
    /// The `expandValues` attributes.
    pub expand_values: Option<Vec<AttributeName>>,
    /// The `jsonKeys` attributes.
    pub json_keys: Option<Vec<AttributeName>>,
}

/// The written lists of the stored expanded ones.
fn written_projection(lists: &[ExpandedProjectionList]) -> Result<Vec<WrittenProjectionList>, ProjectionError> {
    lists
        .iter()
        .map(|list| {
            list.iter()
                .map(WrittenProjectionTerm::try_from)
                .collect::<Result<Vec<_>, _>>()
                .map(WrittenProjectionList::from)
        })
        .collect()
}

/// The written names of the stored expanded ones.
fn written_names(names: &[AttributeIri]) -> Vec<AttributeName> {
    names.iter().cloned().map(AttributeName::from).collect()
}

impl TryFrom<&Subscription> for WrittenSubscriptionTerms {
    type Error = WrittenTermsError;

    fn try_from(subscription: &Subscription) -> Result<WrittenSubscriptionTerms, WrittenTermsError> {
        let Subscription {
            selection,
            filters,
            notification,
            ..
        } = subscription;
        Ok(WrittenSubscriptionTerms {
            entities: selection
                .entities
                .as_deref()
                // clone: the written selectors are owned apart from the stored subscription
                .map(|entities| entities.iter().cloned().map(EntitySelector::into_written).collect::<Result<_, _>>())
                .transpose()?,
            watched_attributes: selection.watched_attributes.as_deref().map(written_names),
            attributes: notification.attributes.as_deref().map(written_names),
            pick: notification.pick.as_deref().map(written_projection).transpose()?,
            omit: notification.omit.as_deref().map(written_projection).transpose()?,
            // clone: the written filter is owned apart from the stored subscription
            q: filters.q.clone().map(Filter::into_written_with_member_spelling).transpose()?,
            // clone: the written geo-query is owned apart from the stored subscription
            geo_q: filters.geo_q.clone().map(GeoConstraint::into_written),
            // clone: the written filter is owned apart from the stored subscription
            csf: filters.csf.clone().map(Filter::into_written_with_member_spelling).transpose()?,
            expand_values: filters.expand_values.as_deref().map(written_names),
            json_keys: filters.json_keys.as_deref().map(written_names),
        })
    }
}
