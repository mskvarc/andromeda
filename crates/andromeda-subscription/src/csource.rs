use crate::{
    change::{CsourceSubscriptionChange, UpsertKind},
    error::SubscriptionError,
    request_validation,
    term_expansion,
    variant::{DatasetIdPolicy, SubscriptionVariant},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{subscription::Subscription, subscription_request::SubscriptionRequest};
use async_trait::async_trait;

/// CSR-subscription-specific behaviour.
pub struct CsrSubscriptionVariant {
    engine: JsonLdEngine,
}

impl CsrSubscriptionVariant {
    /// Construct context-source-subscription-specific lifecycle behavior.
    #[must_use]
    pub const fn new(engine: JsonLdEngine) -> CsrSubscriptionVariant {
        CsrSubscriptionVariant { engine }
    }
}

#[async_trait]
impl SubscriptionVariant for CsrSubscriptionVariant {
    type Change = CsourceSubscriptionChange;

    async fn expand_terms(&self, request: &mut SubscriptionRequest, context: &ContextSpec) -> Result<(), SubscriptionError> {
        term_expansion::expand_common_subscription_terms(&self.engine, request, context).await
    }

    /// A Context Source Registration Subscription is served as it is stored:
    /// its notifications carry a `triggerReason` (clause 5.3.2), so no
    /// `notificationTrigger` default is supplied.
    fn present(&self, _subscription: &mut Subscription) {}

    fn validate_members(&self, request: &SubscriptionRequest) -> Result<(), SubscriptionError> {
        request_validation::validate_no_geo_json_accept(request)
    }

    fn validate_update_extra(&self, _request: &SubscriptionRequest) -> Result<(), SubscriptionError> {
        // Clause 5.11.3.4 checks a fragment only against the data types and
        // restrictions of Table 5.2.12-1, which sets no future-only bound on
        // `expiresAt`, and does not apply the past-expiry refusal of clause
        // 5.8.2.4: a past `expiresAt` is accepted and expires the subscription.
        Ok(())
    }

    fn dataset_id_policy(&self) -> DatasetIdPolicy {
        DatasetIdPolicy::Discard
    }

    fn make_upserted(&self, target: ContextTarget, id: Identifier, kind: UpsertKind) -> CsourceSubscriptionChange {
        CsourceSubscriptionChange::Upserted { target, id, kind }
    }

    fn make_deleted(&self, target: ContextTarget, id: Identifier) -> CsourceSubscriptionChange {
        CsourceSubscriptionChange::Deleted { target, id }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        csource::CsrSubscriptionVariant,
        error::SubscriptionError,
        subscription_validation_reason::SubscriptionValidationReason,
        variant::SubscriptionVariant,
    };
    use andromeda_jsonld::engine::processor::JsonLdEngine;
    use andromeda_ngsi_ld_model::{request_body::RequestBody, subscription_request::SubscriptionRequest};
    use andromeda_ngsi_ld_representation::core_term_aliases::CoreTermAliases;
    use serde_json::json;

    /// Table 5.2.12-1 gives `temporalQ` to Context Registration Subscriptions,
    /// so a Context Source Registration Subscription keeps it.
    #[test]
    fn a_temporal_q_on_a_context_source_registration_subscription_is_admitted() {
        let variant = CsrSubscriptionVariant::new(JsonLdEngine::builder().allow_remote(false).build().expect("engine builds"));
        let wire = json!({"temporalQ": {"timerel": "after", "timeAt": "2020-06-01T00:00:00Z"}});
        let request = SubscriptionRequest::from_request_text(wire.to_string().as_bytes(), CoreTermAliases::keyword_spellings()).expect("a readable request");
        let result = variant.validate_members(&request);
        assert!(result.is_ok(), "got {result:?}");
    }

    fn validated(accept: &str) -> Result<(), SubscriptionError> {
        let variant = CsrSubscriptionVariant::new(JsonLdEngine::builder().allow_remote(false).build().expect("engine builds"));
        let wire = json!({"notification": {"endpoint": {"uri": "https://receiver.example.com/notify", "accept": accept}}});
        let request = SubscriptionRequest::from_request_text(wire.to_string().as_bytes(), CoreTermAliases::keyword_spellings()).expect("a readable request");
        variant.validate_members(&request)
    }

    /// Clause 6.3.9: a csource notification is `application/json` by default
    /// and "can be changed to" `application/ld+json`, and to nothing else.
    #[test]
    fn a_geo_json_accept_on_a_context_source_registration_subscription_is_refused() {
        assert!(matches!(
            validated("application/geo+json"),
            Err(SubscriptionError::Validation(SubscriptionValidationReason::GeoJsonForCsourceSubscription))
        ));
    }

    /// Clause 5.11.3.4 applies no past-expiry refusal to an update fragment.
    #[test]
    fn an_update_with_a_past_expires_at_is_admitted() {
        let variant = CsrSubscriptionVariant::new(JsonLdEngine::builder().allow_remote(false).build().expect("engine builds"));
        let wire = json!({"expiresAt": "2020-01-01T00:00:00Z"});
        let request = SubscriptionRequest::from_request_text(wire.to_string().as_bytes(), CoreTermAliases::keyword_spellings()).expect("a readable request");
        let result = variant.validate_update_extra(&request);
        assert!(result.is_ok(), "got {result:?}");
    }

    #[test]
    fn a_json_or_json_ld_accept_on_a_context_source_registration_subscription_is_admitted() {
        for accept in ["application/json", "application/ld+json"] {
            let result = validated(accept);
            assert!(result.is_ok(), "{accept}: {result:?}");
        }
    }
}
