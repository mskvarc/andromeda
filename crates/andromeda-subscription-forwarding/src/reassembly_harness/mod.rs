//! A broker's distributed subscription side wired to in-memory doubles, for
//! tests of what it does with a notification posted under a forwarded copy's
//! identifier, and with the lifecycle of the local subscription.
//!
//! The scene holds one local subscription delivering to an `http://` endpoint
//! that a loopback receiver serves, so what the subscriber is sent is read off
//! the requests that receiver recorded, the entities this broker holds, the
//! mappings of the copy a source holds, and the registrations through which
//! the other holders of an entity are found.

pub(crate) mod answering_source;
pub(crate) mod forwarding_maps;
pub(crate) mod held_entities;
pub(crate) mod inert_ports;
pub(crate) mod recording_subscriptions;
pub(crate) mod scripted_registrations;
pub(crate) mod stored_forms;
pub(crate) mod subscribing_source;

use crate::{
    error::SubscriptionForwardingError,
    forwarder::ForwardDecision,
    inbound::Inbound,
    reassembly_harness::{
        forwarding_maps::{DiscardingRouter, ForwardingMaps},
        held_entities::HeldEntities,
        inert_ports::{AcceptingEndpoints, NoLocalTypes, offline_engine},
        recording_subscriptions::RecordingSubscriptions,
        scripted_registrations::ScriptedRegistrations,
        stored_forms::{stored_companion, stored_subscription},
    },
    receiver::ReceiverEndpoint,
    service::SubscriptionForwardingService,
};
use andromeda_configuration::schema::{federation::FederationConfig, request_limits::RequestLimitsConfig};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_delivery::{dispatch::TransportDispatcher, http::config::HttpTransportConfig, mqtt::config::MqttPoolConfig};
use andromeda_entity::{
    configured_policy::{filter_complexity_limits, geometry_census_limits},
    query::EntityQueryService,
    spatial_acceleration::SpatialAcceleration,
};
use andromeda_federation::{FederationService, local_source::LocalSource};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::{context_source_pseudonym::ContextSourcePseudonym, identifier::Identifier};
use andromeda_ngsi_ld_model::{registration::ContextSourceRegistration, subscription::Subscription};
use andromeda_ngsi_ld_query::{
    filter::language_targeting::LanguageTargeting,
    geo::{admission::GeoQueryAdmission, near_distance_band::NearDistanceBand},
};
use andromeda_subscription::{
    csource::CsrSubscriptionVariant,
    deleted_members::DeletedMembers,
    error::SubscriptionError,
    filter_admission::SubscriptionFilterAdmission,
    forwarding_hook::{ForwardedCopy, SubscriptionForwardingHook},
    lifecycle::SubscriptionService,
};
use andromeda_tenancy::{context_source_alias::ContextSourceAliases, tenant_ref::TenantRef};
use serde_json::Value;
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
    /// Whether the store holds the companion the mappings link.
    pub(crate) companion: StoredCompanion,
}

/// Whether the store holds the companion the mappings link to the local
/// subscription.
#[derive(Default)]
pub(crate) enum StoredCompanion {
    /// The store holds it.
    #[default]
    Held,
    /// It was deleted underneath its mapping.
    Missing,
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
    /// The endpoint of the local subscription, which the receiver serves.
    endpoint: String,
    /// How many of the receiver's recorded requests `delivered` has returned.
    returned: usize,
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

impl ReassemblyScene {
    /// The scene `spec` describes.
    pub(crate) async fn start(spec: SceneSpec) -> ReassemblyScene {
        let SceneSpec {
            subscription_members,
            held,
            registrations,
            registration_failures,
            copies,
            companion,
        } = spec;
        let receiver = MockServer::start().await;
        Mock::given(method("POST")).respond_with(ResponseTemplate::new(204)).mount(&receiver).await;
        let endpoint = format!("{}/subscriber", receiver.uri());
        let local = stored_subscription(&subscription_members, &endpoint);
        let stored = match companion {
            StoredCompanion::Held => vec![local, stored_companion()],
            StoredCompanion::Missing => vec![local],
        };
        let subscriptions = Arc::new(RecordingSubscriptions::holding(stored));
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
            SubscriptionFilterAdmission {
                geo_q: GeoQueryAdmission::new(geometry_census_limits(&RequestLimitsConfig::default()), NearDistanceBand::Refused),
                q_complexity: filter_complexity_limits(&RequestLimitsConfig::default()),
                language_targeting: LanguageTargeting::Specification,
            },
        );
        let held = Arc::new(HeldEntities::new(held));
        let federation = FederationService::new(
            Arc::new(ScriptedRegistrations::new(registrations, registration_failures)),
            Arc::new(NoLocalTypes),
            offline_engine(),
            LocalSource::new(
                ContextSourceAliases::new(ContextSourcePseudonym::new("andromeda-test").expect("a valid alias")),
                TenantRef::UnnamedDefault,
            ),
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
                geometry_census_limits(&RequestLimitsConfig::default()),
                NearDistanceBand::Refused,
            ))
            .language_targeting(LanguageTargeting::Specification)
            .filter_complexity(filter_complexity_limits(&RequestLimitsConfig::default()))
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
            endpoint,
            returned: 0,
        }
    }

    /// Post `notification`, as a source would post it under the copy it holds,
    /// and let the service handle it under the core context.
    pub(crate) async fn post(&self, notification: &Value) -> Result<(), SubscriptionForwardingError> {
        let text = serde_json::to_vec(notification).expect("the notification writes");
        match Inbound::from_slice(&text).expect("the notification decodes") {
            Inbound::Notification(received) => Box::pin(self.service.handle_notification(&received, &ContextSpec::core_only())).await,
            Inbound::CsourceNotification(received) => Box::pin(self.service.handle_csource_notification(&received)).await,
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
    pub(crate) fn deliveries_succeeded(&self) -> u32 {
        self.subscriptions.successes()
    }

    /// How many times the copies of the local subscription were asked for.
    pub(crate) fn copies_asked(&self) -> usize {
        self.maps.copies_asked()
    }

    /// The subscriptions deleted from the store, companions included.
    pub(crate) fn deleted_subscriptions(&self) -> Vec<Identifier> {
        self.subscriptions.deleted()
    }

    /// The registrations whose copy mapping was removed.
    pub(crate) fn removed_mappings(&self) -> Vec<Identifier> {
        self.maps.removed()
    }

    /// The companion the mappings link to the local subscription.
    pub(crate) fn linked_companion(&self) -> Identifier {
        self.maps.companion()
    }

    /// Whether the store holds a subscription under `id`.
    pub(crate) fn stores(&self, id: &Identifier) -> bool {
        self.subscriptions.holds(id)
    }

    /// Store the local subscription with `members` in place of the defaults,
    /// as a committed update leaves it.
    pub(crate) fn store_local(&self, members: &Value) {
        self.subscriptions.replace(stored_subscription(members, &self.endpoint));
    }

    /// Hold the deletions of subscriptions from now on, the companion's among
    /// them, between their start and their effect.
    pub(crate) fn hold_deletions(&self) {
        self.subscriptions.hold_deletions();
    }

    /// Wait until a held deletion has started.
    pub(crate) async fn deletion_reached(&self) {
        self.subscriptions.deletion_reached().await;
    }

    /// Let the held deletion take effect.
    pub(crate) fn release_deletions(&self) {
        self.subscriptions.release_deletions();
    }

    /// The most companion updates that ran at once.
    pub(crate) fn most_updates_at_once(&self) -> usize {
        self.subscriptions.most_updates_at_once()
    }

    /// The local subscription as the store holds it.
    async fn stored(&self) -> Subscription {
        self.service
            .subscriptions
            .get(&ContextTarget::Live, &local_subscription())
            .await
            .expect("the store answers")
            .expect("the local subscription is stored")
            .subscription
    }

    /// Place a reduced copy of the local subscription at `registration`'s
    /// source the way the subscription's creation places it at a registration
    /// that already matches.
    pub(crate) async fn place_as_at_creation(&self, registration: &ContextSourceRegistration) -> Result<(), SubscriptionForwardingError> {
        self.service
            .apply_forward_decision(&self.stored().await, registration, ForwardDecision::Create, &DeletedMembers::default())
            .await
    }

    /// Tell the service the expiry sweep transitioned the local subscription.
    pub(crate) async fn expire(&self) -> Result<(), SubscriptionError> {
        SubscriptionForwardingHook::expired(&self.service, &local_subscription()).await
    }

    /// Tell the service the local subscription was updated to the state the
    /// store holds, deleting no member.
    pub(crate) async fn update_to_stored(&self) -> Result<(), SubscriptionError> {
        SubscriptionForwardingHook::updated(&self.service, &self.stored().await, &DeletedMembers::default()).await
    }
}
