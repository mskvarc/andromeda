//! The `Testing` entity the ETSI `@context` cases write to make the broker use a stored `@context`: its attributes
//! `A1`, `A2` and `A3` are the terms of the cases' own term object.

use andromeda_test_suite::{broker::BrokerClient, namespace::Namespace, teardown::AnyAnswer, wire};
use googletest::prelude::*;
use iri_rs::IriBuf;
use ngsi_ld::{
    http::{ContextDelivery, operation::CreateEntity},
    model::{Context, Entity, Input},
};
use std::fmt::Display;
use url::Url;

/// The entity `urn:ngsi-ld:Testing:<ns>-<suffix>` of type `Testing`, its `A1`, `A2` and `A3` holding `value of A1`,
/// `value of A2` and `value of A3`.
pub(crate) fn testing_entity(namespace: &Namespace, suffix: impl Display) -> Result<Entity<Input>> {
    Ok(Entity::builder()
        .id(namespace.urn("Testing", suffix)?)
        .entity_type("Testing")
        .property("A1", "value of A1")
        .property("A2", "value of A2")
        .property("A3", "value of A3")
        .build()?)
}

/// Posts `entity` to `/entities/` as `application/ld+json` with the `@context` string `context`, whatever the broker
/// answers (`HTTP *`).
pub(crate) async fn post_in_context(client: &BrokerClient, entity: &Entity<Input>, context: &Url) -> Result<()> {
    let writer = client
        .with_context(Context::from(IriBuf::new(context.to_string())?))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    wire::send_to(&writer, &CreateEntity::builder().entity(entity).build(), "entities/")
        .await
        .any_answer();
    Ok(())
}
