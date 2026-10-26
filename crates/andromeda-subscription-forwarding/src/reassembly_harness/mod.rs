//! A broker's distributed subscription side wired to in-memory doubles, for
//! tests of what it does with a notification posted under a forwarded copy's
//! identifier.
//!
//! The scene holds one local subscription delivering to an `http://` endpoint
//! that a loopback receiver serves, so what the subscriber is sent is read off
//! the requests that receiver recorded, the entities this broker holds, the
//! mappings of the copy a source holds, and the registrations through which
//! the other holders of an entity are found.

pub(crate) mod answering_source;
pub(crate) mod forwarding_maps;
pub(crate) mod held_entities;
pub(crate) mod recording_subscriptions;
pub(crate) mod scripted_registrations;

use crate::{
    error::SubscriptionForwardingError,
    inbound::Inbound,
    reassembly_harness::{
        forwarding_maps::{DiscardingRouter, ForwardingMaps},
        held_entities::HeldEntities,
        recording_subscriptions::RecordingSubscriptions,
        scripted_registrations::ScriptedRegistrations,
    },
    receiver::ReceiverEndpoint,
    service::SubscriptionForwardingService,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_delivery::{dispatch::TransportDispatcher, http::config::HttpTransportConfig, mqtt::config::MqttPoolConfig};
use andromeda_entity::{config::EntityConfig, query::EntityQueryService, spatial_acceleration::SpatialAcceleration};
use andromeda_federation::{FederationService, config::FederationConfig, local_entity_type::LocalEntityTypeResolver};
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::{context_source_pseudonym::ContextSourcePseudonym, entity_type_name::EntityTypeName, identifier::Identifier};
use andromeda_ngsi_ld_model::{key_value_pair::KeyValuePair, registration::ContextSourceRegistration};
use andromeda_ngsi_ld_query::{
    filter::language_targeting::LanguageTargeting,
    geo::{admission::GeoQueryAdmission, near_distance_band::NearDistanceBand},
};
use andromeda_subscription::{
    csource::CsrSubscriptionVariant,
    endpoint_admission::SubscriptionEndpointValidator,
    endpoint_refusal::EndpointRefusal,
    forwarding_hook::ForwardedCopy,
    lifecycle::SubscriptionService,
};
use andromeda_tenancy::tenant_ref::TenantRef;
use futures::future::BoxFuture;
use serde_json::{Value, json};
use std::sync::Arc;
use url::Url;
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::method};

/// The vocabulary a context-free name expands under (clause 5.5.7: the default
/// context), which is how a source spells an entity under the core context.
const DEFAULT_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/default-context/";

/// The expanded IRI the default context gives `name`.
pub(crate) fn default_iri(name: &str) -> String {
    format!("{DEFAULT_CONTEXT}{name}")
}

/// What one scene holds beyond the local subscription's endpoint.
#[derive(Default)]
pub(crate) struct SceneSpec {
    /// Members of the stored local subscription that replace the defaults, as
    /// an object; absent for none.
    pub(crate) subscription_members: Value,
    /// The entities the broker holds.
    pub(crate) held: Vec<EntityState>,
    /// The registrations a retrieval of a split entity finds holders through.
    pub(crate) registrations: Vec<ContextSourceRegistration>,
    /// How many of the first registration queries fail.
    pub(crate) registration_failures: usize,
    /// The copies of the local subscription the sources hold.
    pub(crate) copies: Vec<ForwardedCopy>,
}

/// The distributed subscription side under test and the doubles around it.
pub(crate) struct ReassemblyScene {
    /// The service under test.
    service: SubscriptionForwardingService,
    /// The subscription store, which counts slots claimed and outcomes recorded.
    subscriptions: Arc<RecordingSubscriptions>,
    /// The mappings, which count how often the copies were asked for.
    maps: Arc<ForwardingMaps>,
    /// The loopback receiver the local subscription's endpoint names.
    receiver: MockServer,
    /// How many of the receiver's recorded requests `delivered` has returned.
    returned: usize,
}

/// An endpoint validator for a broker that serves every scheme.
struct AcceptingEndpoints;

impl SubscriptionEndpointValidator for AcceptingEndpoints {
    fn validate(&self, _endpoint: &Url, _notifier_info: Option<&[KeyValuePair]>) -> Result<(), EndpointRefusal> {
        Ok(())
    }
}

/// A resolver for which no entity is held locally.
struct NoLocalTypes;

impl LocalEntityTypeResolver for NoLocalTypes {
    fn local_entity_types<'resolver>(
        &'resolver self,
        _target: &'resolver ContextTarget,
        _id: &'resolver Identifier,
    ) -> BoxFuture<'resolver, Vec<EntityTypeName>> {
        Box::pin(async { Vec::new() })
    }
}

/// An engine that resolves what is embedded, never the network.
fn offline_engine() -> JsonLdEngine {
    JsonLdEngine::builder().allow_remote(false).build().expect("the engine builds")
}

/// The identifier `text` names.
pub(crate) fn identifier(text: &str) -> Identifier {
    text.parse().expect("a valid identifier")
}

/// The local subscription.
pub(crate) fn local_subscription() -> Identifier {
    identifier("urn:ngsi-ld:Subscription:local")
}

/// The copy of the local subscription a source holds.
pub(crate) fn copy_subscription() -> Identifier {
    identifier("urn:ngsi-ld:Subscription:copy")
}

/// The registration of the source that holds the copy and sends its
/// notifications.
pub(crate) fn sender_registration() -> Identifier {
    identifier("urn:ngsi-ld:ContextSourceRegistration:sender")
}

/// The companion subscription mirroring the local one.
pub(crate) fn companion_subscription() -> Identifier {
    identifier("urn:ngsi-ld:Subscription:companion")
}

/// The stored form of the local subscription: it watches `Vehicle`, notifies
/// `endpoint` in the normalized format, and takes `members` over. The
/// endpoint is always the receiver's, whatever `members` say about it.
fn stored_subscription(members: &Value, endpoint: &str) -> Value {
    let mut stored = json!({
        "id": local_subscription().as_str(),
        "type": "Subscription",
        "entities": [{"type": default_iri("Vehicle")}],
        "notification": {"format": "normalized"},
        "status": "active",
        "createdAt": "2020-01-01T00:00:00Z",
        "modifiedAt": "2020-01-01T00:00:00Z"
    });
    if let (Some(base), Some(replacements)) = (stored.as_object_mut(), members.as_object()) {
        base.extend(replacements.iter().map(|(name, value)| (name.clone(), value.clone()))); // clone: the stored form owns its members
    }
    stored["notification"]["endpoint"] = json!({"uri": endpoint});
    stored
}

impl ReassemblyScene {
    /// The scene `spec` describes.
    pub(crate) async fn start(spec: SceneSpec) -> ReassemblyScene {
        let SceneSpec {
            subscription_members,
            held,
            registrations,
            registration_failures,
            copies,
        } = spec;
        let receiver = MockServer::start().await;
        Mock::given(method("POST")).respond_with(ResponseTemplate::new(204)).mount(&receiver).await;
        let endpoint = format!("{}/subscriber", receiver.uri());
        let subscriptions = Arc::new(RecordingSubscriptions::holding(stored_subscription(&subscription_members, &endpoint)));
        let maps = Arc::new(ForwardingMaps::new(
            local_subscription(),
            sender_registration(),
            copy_subscription(),
            companion_subscription(),
            copies,
        ));
        let transport = TransportDispatcher::new(HttpTransportConfig::default(), MqttPoolConfig::default()).expect("the dispatcher builds");
        let csource_subscriptions = SubscriptionService::new(
            subscriptions.clone(), // clone: the scene keeps its handle for the counts
            offline_engine(),
            CsrSubscriptionVariant::new(offline_engine()),
            None,
            Arc::new(AcceptingEndpoints),
            GeoQueryAdmission::new(EntityConfig::default().geometry_census_limits(), NearDistanceBand::Refused),
            LanguageTargeting::Specification,
        );
        let held = Arc::new(HeldEntities::new(held));
        let federation = FederationService::new(
            Arc::new(ScriptedRegistrations::new(registrations, registration_failures)),
            Arc::new(NoLocalTypes),
            offline_engine(),
            ContextSourcePseudonym::new("andromeda-test").expect("a valid alias"),
            &FederationConfig::default(),
        )
        .expect("the federation service builds");
        let entity_query = EntityQueryService::builder()
            .reader(held.clone()) // clone: the same store answers both ports
            .query_store(held)
            .engine(offline_engine())
            .spatial(SpatialAcceleration::StoreVerified)
            .tenant(TenantRef::UnnamedDefault)
            .remote_linked(None)
            .geo_admission(GeoQueryAdmission::new(
                EntityConfig::default().geometry_census_limits(),
                NearDistanceBand::Refused,
            ))
            .language_targeting(LanguageTargeting::Specification)
            .build();
        let service = SubscriptionForwardingService::builder()
            .live_target(ContextTarget::Live)
            .receiver(ReceiverEndpoint::from_advertised(&Url::parse("http://broker.example:1056").expect("a valid origin")).expect("a hierarchical origin"))
            .tenant(TenantRef::UnnamedDefault)
            .subscriptions(subscriptions.clone()) // clone: the scene keeps its handle for the counts
            .csource_subscriptions(Arc::new(csource_subscriptions))
            .mappings(maps.clone()) // clone: the scene keeps its handle for the counts
            .router(Arc::new(DiscardingRouter))
            .federation(Arc::new(federation))
            .entity_query(Arc::new(entity_query))
            .transport(Arc::new(transport))
            .engine(offline_engine())
            .build();
        ReassemblyScene {
            service,
            subscriptions,
            maps,
            receiver,
            returned: 0,
        }
    }

    /// Post `notification`, as a source would post it under the copy it holds,
    /// and let the service handle it under the core context.
    pub(crate) async fn post(&self, notification: &Value) -> Result<(), SubscriptionForwardingError> {
        let text = serde_json::to_vec(notification).expect("the notification writes");
        match Inbound::from_slice(&text).expect("the notification decodes") {
            Inbound::Notification(received) => Box::pin(self.service.handle_notification(&received, &ContextSpec::core_only())).await,
            Inbound::CsourceNotification(received) => Box::pin(self.service.handle_csource_notification(&received, &ContextSpec::core_only())).await,
        }
    }

    /// The notification the local subscriber was sent since the last call, as
    /// the JSON it was written as. A delivery is recorded before its POST is
    /// answered, so every notification `post` has returned for is visible here.
    pub(crate) async fn delivered(&mut self) -> Option<Value> {
        let requests = self.receiver.received_requests().await.expect("the receiver records its requests");
        let next = requests.get(self.returned)?;
        self.returned += 1;
        Some(serde_json::from_slice(&next.body).expect("the delivered notification is JSON"))
    }

    /// How many notification slots of the local subscription were claimed.
    pub(crate) fn slots_claimed(&self) -> usize {
        self.subscriptions.claims()
    }

    /// How many deliveries were recorded as successful.
    pub(crate) fn deliveries_succeeded(&self) -> usize {
        self.subscriptions.successes()
    }

    /// How many times the copies of the local subscription were asked for.
    pub(crate) fn copies_asked(&self) -> usize {
        self.maps.copies_asked()
    }
}
