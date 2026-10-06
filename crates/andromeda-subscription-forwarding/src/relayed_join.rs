//! The Linked Entities the local subscription's `join` reaches from the
//! entities of a relayed notification (clause 4.5.23, clause 5.8.6: "if the
//! notification.join member value is set then Linked Entity retrieval shall be
//! provided").
//!
//! A relayed copy is a notification of the local subscription, so its Linked
//! Entities are read from this broker's store by the engine a locally
//! triggered notification reads them through, and written by the same plan.

use crate::{error::SubscriptionForwardingError, forwarding_invariant::ForwardingInvariant, service::SubscriptionForwardingService};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::query::{
    EntityQueryService,
    notification_join::{NotificationJoin, linked_attribute_index, notification_linked_graph},
};
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_linked_retrieval::{graph::LinkedGraph, join::RetrievingJoin, narrowing::member::MemberRule, plan::JoinPlan, typed::TypedEntities};
use andromeda_ngsi_ld_model::{entity::Entity, notification::NotificationData, subscription::Subscription};
use andromeda_ngsi_ld_query::projection::attribute_iri_index::AttributeIriIndex;
use andromeda_ngsi_ld_render::{
    joined_notification::{join_plan, joined_entities},
    notification_data::{DataShape, notification_data},
    notification_entity::{NotificationEntity, TargetSpelling},
    notification_language::NotificationLanguage,
    structural_projection::StructuralProjection,
    version::{adapted_payload::AdaptedPayload, fallback::VersionFallback},
};
use andromeda_subscription::compiled::CompiledSubscription;

/// The Linked Entities one relayed notification's entities reach, with the
/// expansion of every attribute key the notification carries.
#[derive(Debug)]
pub(crate) struct RelayedLinks {
    /// What the traversal reached.
    graph: LinkedGraph<Entity>,
    /// The expansion of every attribute key of the entities and the graph.
    index: AttributeIriIndex,
}

impl RelayedLinks {
    /// The Linked Entities `entities` reach under the subscription's `join`,
    /// read from `target` and compacted under `context`; `None` when the
    /// subscription names no retrieving join or the links cannot be resolved,
    /// which sends the copy carrying its entities alone, as a locally triggered
    /// notification is sent.
    pub(crate) async fn resolve(
        compiled: &CompiledSubscription,
        entities: &[Entity],
        reading: RelayedJoinSource<'_>,
        context: &ContextSpec,
    ) -> Option<RelayedLinks> {
        RetrievingJoin::from_mode(compiled.notification_join)?;
        let join = NotificationJoin {
            target: reading.target,
            join_level: compiled.notification_join_level,
            sys_attrs: compiled.notification_sys_attrs,
            context,
        };
        let graph = notification_linked_graph(reading.entity_query, entities, join).await?;
        let index = linked_attribute_index(reading.engine, context, compiled.list_in_force(), entities.iter(), &graph).await?;
        Some(RelayedLinks { graph, index })
    }

    /// How the subscription's `join` writes these Linked Entities.
    pub(crate) fn plan<'a>(&'a self, compiled: &'a CompiledSubscription) -> Option<JoinPlan<'a, TypedEntities>> {
        join_plan(
            compiled.notification_join,
            Some(&self.graph),
            MemberRule::new(compiled.list_in_force(), &self.index),
        )
    }
}

impl SubscriptionForwardingService {
    /// The `data` of the copy sent to the local subscription's endpoint, its
    /// Entities written as a notification of that subscription writes a local
    /// entity (clause 5.8.6): its `format`, `pick` and `omit`, decoded
    /// Relationship targets, its `lang`, the Linked Entities of its `join`, and
    /// amended to its `ngsildConformance`. Table 5.3.1-1: the local endpoint's
    /// `accept` decides the shape, whatever shape the source sent.
    ///
    /// # Errors
    ///
    /// Returns [`SubscriptionForwardingError::Internal`] when an entity cannot
    /// be written.
    pub(crate) async fn written_relay(
        &self,
        compiled: &CompiledSubscription,
        subscription: &Subscription,
        local_context: &ContextSpec,
        data: &[Entity],
        shape: DataShape<'_>,
    ) -> Result<NotificationData, SubscriptionForwardingError> {
        let format = subscription.notification.format.unwrap_or_default();
        let fallback = compiled
            .ngsild_conformance
            .and_then(|target| VersionFallback::new(target, AdaptedPayload::Entity));
        let projection = StructuralProjection::from(&compiled.notification_projection);
        // Table 5.2.12-1 `lang`: clause 4.15 reduces every `LanguageProperty`
        // of a notified entity under the subscription's language, relayed or not.
        let language = subscription.delivery.lang.as_ref().map(NotificationLanguage::of);
        // Clause 5.8.6: "If the notification.join member value is set then
        // Linked Entity retrieval shall be provided", for a relayed copy as for
        // a notification the broker triggers itself.
        let source = RelayedJoinSource {
            target: &self.live_target,
            entity_query: &self.entity_query,
            engine: &self.engine,
        };
        let links = RelayedLinks::resolve(compiled, data, source, local_context).await;
        let plan = links.as_ref().and_then(|links| links.plan(compiled));
        let relayed = joined_entities(
            plan.as_ref(),
            data,
            |entity, cursor| {
                let relayed = NotificationEntity::relayed(entity, format, projection, language.as_ref()).with_fallback(fallback);
                match cursor {
                    Some(cursor) => relayed.with_inline_join(cursor),
                    None => relayed,
                }
            },
            |linked| {
                NotificationEntity::linked(linked, format, TargetSpelling::Decoded)
                    .with_fallback(fallback)
                    .with_language(language.as_ref())
            },
        );
        Ok(notification_data(&relayed, shape).map_err(ForwardingInvariant::Rendering)?)
    }
}

/// What the Linked Entities of a relayed notification are read through.
#[derive(Clone, Copy)]
pub(crate) struct RelayedJoinSource<'a> {
    /// The branch the Linked Entities are read from.
    pub(crate) target: &'a ContextTarget,
    /// The store's read side.
    pub(crate) entity_query: &'a EntityQueryService,
    /// The tenant's JSON-LD processor.
    pub(crate) engine: &'a JsonLdEngine,
}
