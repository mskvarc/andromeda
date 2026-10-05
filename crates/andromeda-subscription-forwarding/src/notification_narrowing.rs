//! Narrowing a re-assembled entity to what the local subscription delivers
//! (clause 5.8.6 `notification.attributes`, `pick` and `omit`).

use crate::error::SubscriptionForwardingError;
use andromeda_entity::query::{
    attribute_expansion::AttributeExpansions,
    attribute_projection::{filter_entity_attrs, project_entity},
};
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_subscription::compiled::CompiledSubscription;
use iri_rs::IriBuf;

/// The attributes the local subscription's `notification.attributes` names,
/// as the IRIs a narrowing compares against.
///
/// Every stored name is an absolute IRI because creation expanded it
/// (clause 5.5.7); one that is not names no attribute.
pub(crate) fn notification_attribute_iris(compiled: &CompiledSubscription) -> Option<Vec<IriBuf>> {
    compiled
        .notification_attributes
        .as_deref()
        .map(|attributes| attributes.iter().filter_map(|name| IriBuf::new(name.as_str().to_owned()).ok()).collect())
}

/// Narrow one re-assembled entity to what the local subscription delivers
/// (clause 5.8.6 `notification.attributes`, `pick` and `omit`) -- the same
/// clause 4.21 projection a query applies, so a subscriber sees the entity a
/// query under the same selection would show.
pub(crate) async fn narrow_entity(
    entity: &mut Entity,
    compiled: &CompiledSubscription,
    wanted: Option<&[IriBuf]>,
    expansions: &mut AttributeExpansions<'_>,
) -> Result<(), SubscriptionForwardingError> {
    if let Some(wanted) = wanted {
        filter_entity_attrs(entity, wanted, expansions).await?;
    }
    project_entity(entity, &compiled.notification_projection, expansions).await?;
    Ok(())
}
