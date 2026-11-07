//! The keywords a rule tests a key for.
//!
//! A key whose type is a set of keywords, such as a `mode`, is compared by
//! the keyword it holds. One enum spans every such type, so a rule held as
//! data names a keyword of any of them, and a refusal names it the way the
//! file writes it.

use crate::schema::{
    feature_mode::FeatureMode,
    federation::cooldown::CooldownMode,
    http::{
        cors::{CorsMode, CorsPermission},
        forwarded::ForwardedTrust,
    },
    http3::Http3Mode,
    listener_transport::ListenerTransport,
    management::ManagementMode,
    tls::ClientAuthenticationMode,
};
use derive_more::{Display, From};

/// A keyword a key holds, displayed as the file writes it.
///
/// A rule compares a key only with keywords of the key's own type.
#[derive(Clone, Copy, Debug, Display, Eq, From, PartialEq)]
pub enum ChoiceValue {
    /// A feature switch: `"enabled"` or `"disabled"`.
    #[display("{_0}")]
    Feature(FeatureMode),
    /// Where the management endpoints are served.
    #[display("{_0}")]
    Management(ManagementMode),
    /// What a TCP listener speaks.
    #[display("{_0}")]
    Transport(ListenerTransport),
    /// Whether HTTP/3 is served.
    #[display("{_0}")]
    Http3(Http3Mode),
    /// Whether a client certificate is asked for.
    #[display("{_0}")]
    ClientAuthentication(ClientAuthenticationMode),
    /// Which CORS policy applies.
    #[display("{_0}")]
    Cors(CorsMode),
    /// Whether CORS grants what a browser asks for.
    #[display("{_0}")]
    CorsPermission(CorsPermission),
    /// Whose forwarded fields are believed.
    #[display("{_0}")]
    ForwardedTrust(ForwardedTrust),
    /// How a failing context source is held back.
    #[display("{_0}")]
    Cooldown(CooldownMode),
    /// A CORS list written as the wildcard `"any"` instead of its entries.
    #[display("any")]
    #[from(skip)]
    Any,
}

#[cfg(test)]
mod tests {
    use crate::{
        schema::{http::forwarded::ForwardedTrust, management::ManagementMode},
        validate::choice_value::ChoiceValue,
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn a_keyword_reads_as_the_file_writes_it() {
        assert_eq!(
            [
                ChoiceValue::from(ManagementMode::SeparatePort),
                ChoiceValue::from(ForwardedTrust::Proxies),
                ChoiceValue::Any
            ]
            .map(|choice| choice.to_string()),
            ["separate-port", "proxies", "any"]
        );
    }
}
