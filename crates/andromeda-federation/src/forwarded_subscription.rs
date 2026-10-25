//! Spelling a reduced subscription in a registration's forwarding context
//! (clauses 4.3.6.6 and 5.8.1.4).
//!
//! The copy a source receives is the typed [`SubscriptionRequest`] the broker
//! reduced, so every name it carries is a typed value and is respelled as one:
//! the entity selectors' types, `watchedAttributes`, `notification.attributes`,
//! `q` and the geoquery's `geoproperty`. Every other member holds no name and
//! is unchanged; the reduced copy carries neither `jsonKeys` nor
//! `expandValues`.

use crate::{error::FederationError, forwarded_names::ForwardedNames};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_model::{clearable::Clearable, subscription_request::SubscriptionRequest};

impl ForwardedNames<'_> {
    /// `request` with every name it carries spelled in the forwarding context.
    ///
    /// # Errors
    ///
    /// Returns the errors of [`ForwardedNames::attr`], [`ForwardedNames::type_expr`],
    /// [`ForwardedNames::q`] and [`ForwardedNames::geoproperty`] when one of
    /// its names does not reword.
    pub(crate) async fn subscription(&self, request: &SubscriptionRequest) -> Result<SubscriptionRequest, FederationError> {
        let mut respelled = request.clone(); // clone: the forwarded copy is a second request, the reduced one is kept
        if let Some(selectors) = respelled.entities.as_set_mut() {
            for selector in selectors {
                selector.type_ = self.type_expr(&selector.type_).await?;
            }
        }
        if let Some(watched) = respelled.watched_attributes.as_set() {
            respelled.watched_attributes = Clearable::Set(self.attributes(watched).await?);
        }
        if let Some(notification) = respelled.notification.as_mut()
            && let Some(attributes) = notification.attributes.as_deref()
        {
            notification.attributes = Some(self.attributes(attributes).await?);
        }
        if let Some(expression) = respelled.q.as_set() {
            respelled.q = Clearable::Set(self.q(expression, None).await?);
        }
        if let Clearable::Set(geo) = &mut respelled.geo_q
            && let Some(property) = geo.geoproperty.as_ref()
        {
            geo.geoproperty = Some(self.geoproperty(property).await?);
        }
        Ok(respelled)
    }

    /// The attribute names `names` spelled in the forwarding context, in order.
    async fn attributes(&self, names: &[AttributeName]) -> Result<Vec<AttributeName>, FederationError> {
        let mut respelled = Vec::with_capacity(names.len());
        for name in names {
            respelled.push(self.attr(name).await?);
        }
        Ok(respelled)
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{forwarded_names::ForwardedNames, forwarding_context::Rewording, term_resolver::TermResolver};
    use andromeda_json_literal::JsonLiteral;
    use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
    use andromeda_ngsi_ld_model::subscription_request::SubscriptionRequest;
    use serde_json::json;

    fn context(members: &serde_json::Value) -> ContextSpec {
        ContextSpec::from_body(JsonLiteral::admit(&members.to_string()).expect("an admissible literal"))
    }

    /// Every name of a reduced subscription is respelled from its typed
    /// members, and what holds no name is untouched.
    #[tokio::test]
    async fn a_subscription_is_spelled_from_its_typed_members() {
        let engine = JsonLdEngine::builder().allow_remote(false).build().expect("engine builds");
        let inbound = context(&json!({"speed": "https://example.org/speed", "Vehicle": "https://example.org/Vehicle", "brand": "https://example.org/brand"}));
        let naming = context(&json!({"velocity": "https://example.org/speed", "Car": "https://example.org/Vehicle"}));
        let resolver = TermResolver::new(&engine, &inbound);
        let names = ForwardedNames::prepare(&engine, &resolver, &naming, Rewording::Reword)
            .await
            .expect("the forwarding context processes");
        let request: SubscriptionRequest = serde_json::from_value(json!({
            "id": "urn:ngsi-ld:Subscription:forwarded-1",
            "type": "Subscription",
            "entities": [{"type": "Vehicle"}],
            "watchedAttributes": ["speed"],
            "q": "speed>10",
            "geoQ": {"geometry": "Point", "coordinates": [1, 2], "georel": "near;maxDistance==10", "geoproperty": "speed"},
            "notification": {"attributes": ["speed", "brand"], "endpoint": {"uri": "http://receiver.example.org/notify"}}
        }))
        .expect("a subscription request");

        let respelled = serde_json::to_value(names.subscription(&request).await.expect("rewords")).expect("the request writes");

        pretty_assertions::assert_eq!(respelled["entities"][0]["type"], json!("Car"));
        pretty_assertions::assert_eq!(respelled["watchedAttributes"], json!(["velocity"]));
        pretty_assertions::assert_eq!(respelled["q"], json!("velocity>10"));
        pretty_assertions::assert_eq!(respelled["geoQ"]["geoproperty"], json!("velocity"));
        pretty_assertions::assert_eq!(respelled["notification"]["attributes"], json!(["velocity", "https://example.org/brand"]));
        pretty_assertions::assert_eq!(respelled["notification"]["endpoint"]["uri"], json!("http://receiver.example.org/notify"));
        pretty_assertions::assert_eq!(respelled["type"], json!("Subscription"));
    }

    /// With the forwarding context the inbound one, the copy travels as it is.
    #[tokio::test]
    async fn a_subscription_in_the_inbound_context_is_unchanged() {
        let engine = JsonLdEngine::builder().allow_remote(false).build().expect("engine builds");
        let inbound = context(&json!({"speed": "https://example.org/speed"}));
        let resolver = TermResolver::new(&engine, &inbound);
        let names = ForwardedNames::prepare(&engine, &resolver, &inbound, Rewording::AsInbound)
            .await
            .expect("the forwarding context processes");
        let request: SubscriptionRequest = serde_json::from_value(json!({"watchedAttributes": ["speed"], "q": "speed>10"})).expect("a request");
        let kept = serde_json::to_value(names.subscription(&request).await.expect("keeps")).expect("the request writes");
        pretty_assertions::assert_eq!(kept, json!({"watchedAttributes": ["speed"], "q": "speed>10"}));
    }
}
