//! Narrowing a re-assembled entity to what the local subscription delivers
//! (clause 5.8.6 `notification.attributes`, `pick` and `omit`).

use crate::error::SubscriptionForwardingError;
use andromeda_entity::query::{
    attribute_expansion::AttributeExpansions,
    attribute_projection::{filter_entity_attrs, project_entity},
};
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_subscription::compiled::CompiledSubscription;

/// Narrow one re-assembled entity to what the local subscription delivers
/// (clause 5.8.6 `notification.attributes`, `pick` and `omit`) -- the same
/// clause 4.21 projection a query applies, so a subscriber sees the entity a
/// query under the same selection would show.
pub(crate) async fn narrow_entity(
    entity: &mut Entity,
    compiled: &CompiledSubscription,
    expansions: &mut AttributeExpansions<'_>,
) -> Result<(), SubscriptionForwardingError> {
    if !compiled.notification_attributes.is_empty() {
        filter_entity_attrs(entity, &compiled.notification_attributes, expansions).await?;
    }
    project_entity(entity, &compiled.notification_projection, expansions).await?;
    Ok(())
}
