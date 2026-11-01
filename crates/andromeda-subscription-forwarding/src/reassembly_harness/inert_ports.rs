//! The collaborators a scene needs and never exercises: an endpoint validator
//! accepting every scheme, a resolver for which nothing is held locally, and a
//! JSON-LD engine that never reaches the network.

use andromeda_context_space::context_target::ContextTarget;
use andromeda_federation::local_entity_type::LocalEntityTypeResolver;
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, identifier::Identifier};
use andromeda_ngsi_ld_model::key_value_pair::KeyValuePair;
use andromeda_subscription::{endpoint_admission::SubscriptionEndpointValidator, endpoint_refusal::EndpointRefusal};
use futures::future::BoxFuture;
use url::Url;

/// An endpoint validator for a broker that serves every scheme.
pub(crate) struct AcceptingEndpoints;

impl SubscriptionEndpointValidator for AcceptingEndpoints {
    fn validate(&self, _endpoint: &Url, _notifier_info: Option<&[KeyValuePair]>, _receiver_info: Option<&[KeyValuePair]>) -> Result<(), EndpointRefusal> {
        Ok(())
    }
}

/// A resolver for which no entity is held locally.
pub(crate) struct NoLocalTypes;

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
pub(crate) fn offline_engine() -> JsonLdEngine {
    JsonLdEngine::builder().allow_remote(false).build().expect("the engine builds")
}
