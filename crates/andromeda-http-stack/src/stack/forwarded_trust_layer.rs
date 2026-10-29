//! Deciding, per request, whether its forwarded fields may be believed.
//!
//! The decision rests on the peer the request arrived from, which the
//! listener installs as a `ConnectInfo<SocketAddr>` request extension. An
//! IPv4 client reaching a dual-stack listener arrives as an IPv4-mapped IPv6
//! address, so a peer is matched as the address it canonically is.

use andromeda_configuration::{
    address_rule::AddressRule,
    schema::http::forwarded::{ForwardedConfig, ForwardedTrust},
};
use andromeda_http_extraction::forwarded_field_trust::ForwardedFieldTrust;
use axum::{
    extract::{ConnectInfo, Request, State},
    middleware::Next,
    response::Response,
};
use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
};

/// Whose forwarded fields are believed.
#[derive(Clone, Debug)]
pub(crate) enum ForwardedTrustPolicy {
    /// No peer's.
    Nobody,
    /// Every peer's.
    Anyone,
    /// Only the fields of a peer inside one of the listed proxies.
    Proxies(Arc<[AddressRule]>),
}

impl From<&ForwardedConfig> for ForwardedTrustPolicy {
    fn from(config: &ForwardedConfig) -> ForwardedTrustPolicy {
        match config.trust {
            ForwardedTrust::None => ForwardedTrustPolicy::Nobody,
            ForwardedTrust::Any => ForwardedTrustPolicy::Anyone,
            ForwardedTrust::Proxies => ForwardedTrustPolicy::Proxies(Arc::from(config.proxies.as_slice())),
        }
    }
}

impl ForwardedTrustPolicy {
    /// Whether the forwarded fields of a request from `peer` are believed.
    ///
    /// A request whose peer is unknown is believed only by
    /// [`ForwardedTrustPolicy::Anyone`], which asks nothing of the peer.
    fn trust_for(&self, peer: Option<IpAddr>) -> ForwardedFieldTrust {
        match (self, peer.map(|peer| peer.to_canonical())) {
            (ForwardedTrustPolicy::Anyone, Some(_) | None) => ForwardedFieldTrust::Trusted,
            (ForwardedTrustPolicy::Proxies(proxies), Some(peer)) if proxies.iter().any(|proxy| proxy.contains(&peer)) => ForwardedFieldTrust::Trusted,
            (ForwardedTrustPolicy::Nobody | ForwardedTrustPolicy::Proxies(_), Some(_) | None) => ForwardedFieldTrust::Untrusted,
        }
    }
}

/// Installs on `request` whether its forwarded fields are believed, so the
/// origin extractor reads a decision rather than making one.
pub(crate) async fn install_forwarded_trust(State(policy): State<ForwardedTrustPolicy>, mut request: Request, next: Next) -> Response {
    let peer = request.extensions().get::<ConnectInfo<SocketAddr>>().map(|ConnectInfo(address)| address.ip());
    request.extensions_mut().insert(policy.trust_for(peer));
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use crate::stack::forwarded_trust_layer::{ForwardedTrustPolicy, install_forwarded_trust};
    use andromeda_configuration::schema::http::forwarded::{ForwardedConfig, ForwardedTrust};
    use andromeda_http_extraction::forwarded_field_trust::ForwardedFieldTrust;
    use axum::{
        Extension,
        Router,
        body::{Body, to_bytes},
        extract::{ConnectInfo, Request},
        middleware::from_fn_with_state,
        routing::get,
    };
    use pretty_assertions::assert_eq;
    use std::{
        error::Error,
        net::{IpAddr, SocketAddr},
    };
    use tower::ServiceExt;

    /// The policy a `[http.forwarded]` table with `trust` and `proxies` builds.
    fn policy(trust: ForwardedTrust, proxies: &[&str]) -> Result<ForwardedTrustPolicy, Box<dyn Error>> {
        let proxies = proxies.iter().map(|proxy| proxy.parse()).collect::<Result<_, _>>()?;
        Ok(ForwardedTrustPolicy::from(&ForwardedConfig { trust, proxies }))
    }

    /// The trust the policy decides for a request from `peer`.
    fn trust(policy: &ForwardedTrustPolicy, peer: &str) -> Result<ForwardedFieldTrust, Box<dyn Error>> {
        Ok(policy.trust_for(Some(peer.parse::<IpAddr>()?)))
    }

    #[test]
    fn no_peer_is_believed_under_none() -> Result<(), Box<dyn Error>> {
        let policy = policy(ForwardedTrust::None, &[])?;
        assert_eq!(trust(&policy, "10.0.0.1")?, ForwardedFieldTrust::Untrusted);
        assert_eq!(policy.trust_for(None), ForwardedFieldTrust::Untrusted);
        Ok(())
    }

    #[test]
    fn every_peer_is_believed_under_any_even_an_unknown_one() -> Result<(), Box<dyn Error>> {
        let policy = policy(ForwardedTrust::Any, &[])?;
        assert_eq!(trust(&policy, "203.0.113.9")?, ForwardedFieldTrust::Trusted);
        assert_eq!(trust(&policy, "2001:db8::1")?, ForwardedFieldTrust::Trusted);
        assert_eq!(policy.trust_for(None), ForwardedFieldTrust::Trusted);
        Ok(())
    }

    #[test]
    fn only_a_listed_peer_is_believed_under_proxies() -> Result<(), Box<dyn Error>> {
        let policy = policy(ForwardedTrust::Proxies, &["10.0.0.0/8", "192.0.2.7", "fd00::/8"])?;
        for peer in ["10.1.2.3", "192.0.2.7", "fd00::1"] {
            assert_eq!(trust(&policy, peer)?, ForwardedFieldTrust::Trusted, "{peer} is listed");
        }
        for peer in ["11.0.0.1", "192.0.2.8", "2001:db8::1"] {
            assert_eq!(trust(&policy, peer)?, ForwardedFieldTrust::Untrusted, "{peer} is not listed");
        }
        assert_eq!(policy.trust_for(None), ForwardedFieldTrust::Untrusted);
        Ok(())
    }

    /// A dual-stack listener reports an IPv4 client as `::ffff:a.b.c.d`.
    #[test]
    fn an_ipv4_mapped_peer_is_matched_as_the_ipv4_it_embeds() -> Result<(), Box<dyn Error>> {
        let policy = policy(ForwardedTrust::Proxies, &["10.0.0.0/8"])?;
        assert_eq!(trust(&policy, "::ffff:10.1.2.3")?, ForwardedFieldTrust::Trusted);
        assert_eq!(trust(&policy, "::ffff:11.1.2.3")?, ForwardedFieldTrust::Untrusted);
        Ok(())
    }

    /// The decision installed for a request from `peer`, as the handler sees it.
    async fn installed(policy: ForwardedTrustPolicy, peer: Option<SocketAddr>) -> Result<String, Box<dyn Error>> {
        let router = Router::new()
            .route(
                "/probe",
                get(|Extension(trust): Extension<ForwardedFieldTrust>| async move { format!("{trust:?}") }),
            )
            .layer(from_fn_with_state(policy, install_forwarded_trust));
        let mut request = Request::get("/probe").body(Body::empty())?;
        if let Some(peer) = peer {
            request.extensions_mut().insert(ConnectInfo(peer));
        }
        let body = to_bytes(router.oneshot(request).await?.into_body(), usize::MAX).await?;
        Ok(String::from_utf8(body.to_vec())?)
    }

    #[tokio::test]
    async fn the_layer_installs_the_decision_for_the_connection_peer() -> Result<(), Box<dyn Error>> {
        let proxies = policy(ForwardedTrust::Proxies, &["10.0.0.0/8"])?;
        assert_eq!(installed(proxies.clone(), Some("10.0.0.5:40000".parse()?)).await?, "Trusted");
        assert_eq!(installed(proxies.clone(), Some("203.0.113.9:40000".parse()?)).await?, "Untrusted");
        assert_eq!(installed(proxies, None).await?, "Untrusted");
        Ok(())
    }
}
