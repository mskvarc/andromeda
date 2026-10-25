//! Cross-Origin Resource Sharing policy for browser-facing clients.

use andromeda_configuration_scalars::human_duration::HumanDuration;
use axum::http::{HeaderName, Method};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as DeError};
use strum::{Display, EnumString};
use toml_comment::TomlComment;
use url::Url;

/// The browser-origin policy the broker installs at its edge.
///
/// `permissive` and `very-permissive` are complete profiles that ignore every
/// other `[http.cors]` field; `enabled` builds the policy from the custom fields;
/// `disabled` installs no CORS middleware at all.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, Display, EnumString, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
pub enum CorsMode {
    /// Install no CORS middleware.
    Disabled,
    /// Build the CORS policy from the custom `[http.cors]` fields.
    Enabled,
    /// Accept any origin, method and header, without credentials.
    #[default]
    Permissive,
    /// Mirror the request's origin, method and headers, and allow credentials.
    VeryPermissive,
}

/// One HTTP method allowed through CORS, serialised as its token.
///
/// A thin wrapper giving [`Method`] the string-shaped serde representation the
/// configuration needs; [`Method`] itself is not `serde`-serialisable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorsMethod(Method);

impl CorsMethod {
    /// The wrapped HTTP method.
    #[must_use]
    pub fn method(&self) -> Method {
        self.0.clone()
    }
}

impl Serialize for CorsMethod {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.0.as_str())
    }
}

impl<'de> Deserialize<'de> for CorsMethod {
    fn deserialize<D>(deserializer: D) -> Result<CorsMethod, D::Error>
    where
        D: Deserializer<'de>,
    {
        let token = String::deserialize(deserializer)?;
        Method::try_from(token.as_str()).map(CorsMethod).map_err(DeError::custom)
    }
}

/// One HTTP header name allowed or exposed through CORS, serialised as its token.
///
/// A thin wrapper giving [`HeaderName`] the string-shaped serde representation the
/// configuration needs; [`HeaderName`] itself is not `serde`-serialisable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorsHeaderName(HeaderName);

impl CorsHeaderName {
    /// The wrapped header name.
    #[must_use]
    pub fn header_name(&self) -> HeaderName {
        self.0.clone()
    }
}

impl Serialize for CorsHeaderName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.0.as_str())
    }
}

impl<'de> Deserialize<'de> for CorsHeaderName {
    fn deserialize<D>(deserializer: D) -> Result<CorsHeaderName, D::Error>
    where
        D: Deserializer<'de>,
    {
        let token = String::deserialize(deserializer)?;
        HeaderName::try_from(token.as_str()).map(CorsHeaderName).map_err(DeError::custom)
    }
}

/// The wire shape a CORS rule takes: a single keyword token or an explicit list.
///
/// Deserialised untagged so a rule reads as either a string (`"any"`, `"mirror"`)
/// or an array of concrete values, without a tag key.
#[derive(Deserialize)]
#[serde(untagged)]
enum RuleRepr<T> {
    /// A keyword such as `"any"` or `"mirror"`.
    Token(String),
    /// An explicit list of concrete values.
    List(Vec<T>),
}

/// Generate a CORS rule enum with its untagged (keyword | list) serde handling.
///
/// Each rule is either one of a fixed set of keyword variants (rendered as and
/// parsed from a bare string) or a `List` of concrete values (rendered as and
/// parsed from an array). The macro exists because the four rules differ only in
/// their value type and which keywords they accept, and hand-writing four pairs
/// of `Serialize`/`Deserialize` impls would duplicate that shape verbatim.
macro_rules! cors_rule {
    (
        $(#[$enum_doc:meta])*
        enum $name:ident over $elem:ty {
            $(
                $(#[$variant_doc:meta])*
                $variant:ident = $token:literal
            ),+ $(,)?
        }
    ) => {
        $(#[$enum_doc])*
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub enum $name {
            $(
                $(#[$variant_doc])*
                $variant,
            )+
            /// An explicit allow-list of concrete values.
            List(Vec<$elem>),
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<$name, D::Error>
            where
                D: Deserializer<'de>,
            {
                match RuleRepr::<$elem>::deserialize(deserializer)? {
                    RuleRepr::Token(token) => match token.as_str() {
                        $($token => Ok($name::$variant),)+
                        other => Err(DeError::custom(format!(
                            "expected an array of values or one of [{}], found `{other}`",
                            [$($token),+].join(", "),
                        ))),
                    },
                    RuleRepr::List(values) => Ok($name::List(values)),
                }
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                match self {
                    $($name::$variant => serializer.serialize_str($token),)+
                    $name::List(values) => values.serialize(serializer),
                }
            }
        }
    };
}

cors_rule! {
    /// Which origins CORS accepts: any origin, or an explicit allow-list.
    enum OriginRule over Url {
        /// Accept requests from any origin (`Access-Control-Allow-Origin: *`).
        Any = "any"
    }
}

cors_rule! {
    /// Which methods CORS allows: any, the request's own (mirrored), or a list.
    enum MethodRule over CorsMethod {
        /// Allow any method (`Access-Control-Allow-Methods: *`).
        Any = "any",
        /// Reflect the method the request asks for.
        Mirror = "mirror"
    }
}

cors_rule! {
    /// Which request headers CORS allows: any, the request's own, or a list.
    enum HeaderRule over CorsHeaderName {
        /// Allow any request header (`Access-Control-Allow-Headers: *`).
        Any = "any",
        /// Reflect the headers the request asks for.
        Mirror = "mirror"
    }
}

cors_rule! {
    /// Which response headers CORS exposes: any, or an explicit list.
    enum ExposeRule over CorsHeaderName {
        /// Expose any response header (`Access-Control-Expose-Headers: *`).
        Any = "any"
    }
}

/// CORS policy configuration for the broker edge.
#[derive(Debug, Clone, Deserialize, Serialize, TomlComment, PartialEq, Eq)]
pub struct CorsConfig {
    /// The overall CORS mode. `permissive` and `very-permissive` ignore every
    /// other field in this table.
    #[toml_comment(inline)]
    pub mode: CorsMode,
    /// Origins accepted when `mode = enabled`.
    #[toml_comment(inline)]
    pub allow_origins: OriginRule,
    /// Methods allowed when `mode = enabled`.
    #[toml_comment(inline)]
    pub allow_methods: MethodRule,
    /// Request headers allowed when `mode = enabled`.
    #[toml_comment(inline)]
    pub allow_headers: HeaderRule,
    /// Response headers exposed to the browser when `mode = enabled`.
    #[toml_comment(inline)]
    pub expose_headers: ExposeRule,
    /// Whether credentialed requests (cookies, HTTP auth) are permitted. The
    /// Fetch standard forbids combining this with an `any` wildcard, which the
    /// broker rejects at configuration time rather than letting the CORS layer
    /// panic.
    pub allow_credentials: bool,
    /// How long a browser may cache a preflight result. Omitted by default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_age: Option<HumanDuration>,
    /// Whether the `Access-Control-Allow-Private-Network` response is granted for
    /// private-network preflights.
    pub allow_private_network: bool,
}

impl Default for CorsConfig {
    fn default() -> CorsConfig {
        CorsConfig {
            mode: CorsMode::Permissive,
            allow_origins: OriginRule::Any,
            allow_methods: MethodRule::Any,
            allow_headers: HeaderRule::Any,
            expose_headers: ExposeRule::List(Vec::new()),
            allow_credentials: false,
            max_age: None,
            allow_private_network: false,
        }
    }
}

impl CorsConfig {
    /// Whether this policy pairs credentials with a wildcard the Fetch standard
    /// forbids and `tower-http`'s CORS layer panics on.
    ///
    /// Only the literal `any` wildcard conflicts: `mirror` reflects a concrete
    /// value per request and is valid with credentials, matching the CORS layer's
    /// own `is_wildcard` test. The check applies only to `mode = enabled`, since
    /// the profile modes never read these fields.
    #[must_use]
    pub const fn has_credentialed_wildcard(&self) -> bool {
        if !matches!(self.mode, CorsMode::Enabled) || !self.allow_credentials {
            return false;
        }
        matches!(self.allow_origins, OriginRule::Any)
            || matches!(self.allow_methods, MethodRule::Any)
            || matches!(self.allow_headers, HeaderRule::Any)
            || matches!(self.expose_headers, ExposeRule::Any)
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::config::cors::{CorsConfig, CorsMode, ExposeRule, HeaderRule, MethodRule, OriginRule};

    #[derive(serde::Deserialize)]
    struct RuleHolder {
        origins: OriginRule,
        methods: MethodRule,
        headers: HeaderRule,
        expose: ExposeRule,
    }

    #[derive(serde::Deserialize)]
    struct ModeHolder {
        mode: CorsMode,
    }

    #[test]
    fn cors_mode_round_trips_through_toml() {
        for token in ["disabled", "enabled", "permissive", "very-permissive"] {
            let holder: ModeHolder = toml::from_str(&format!("mode = \"{token}\"")).expect("mode parses");
            pretty_assertions::assert_eq!(holder.mode.to_string(), token);
        }
    }

    #[test]
    fn origin_method_header_rules_parse_any_mirror_and_list() {
        let holder: RuleHolder = toml::from_str(
            r#"
origins = ["https://ui.example"]
methods = "mirror"
headers = "any"
expose = ["x-trace-id"]
"#,
        )
        .expect("rules parse");
        pretty_assertions::assert_eq!(holder.methods, MethodRule::Mirror);
        pretty_assertions::assert_eq!(holder.headers, HeaderRule::Any);
        match holder.origins {
            OriginRule::List(origins) => pretty_assertions::assert_eq!(origins.len(), 1),
            OriginRule::Any => panic!("origins should have parsed as a list"),
        }
        match holder.expose {
            ExposeRule::List(headers) => pretty_assertions::assert_eq!(headers.len(), 1),
            ExposeRule::Any => panic!("expose should have parsed as a list"),
        }
    }

    #[test]
    fn cors_enabled_credentials_with_wildcard_is_flagged() {
        let config = CorsConfig {
            mode: CorsMode::Enabled,
            allow_credentials: true,
            allow_origins: OriginRule::Any,
            ..CorsConfig::default()
        };
        assert!(config.has_credentialed_wildcard());
    }

    #[test]
    fn cors_enabled_credentials_with_explicit_origins_is_not_flagged() {
        let config = CorsConfig {
            mode: CorsMode::Enabled,
            allow_credentials: true,
            allow_origins: OriginRule::List(vec!["https://ui.example".parse().expect("origin URL")]),
            allow_methods: MethodRule::List(Vec::new()),
            allow_headers: HeaderRule::List(Vec::new()),
            expose_headers: ExposeRule::List(Vec::new()),
            ..CorsConfig::default()
        };
        assert!(!config.has_credentialed_wildcard());
    }

    #[test]
    fn permissive_mode_never_flags_a_credentialed_wildcard() {
        // The profile modes ignore the credential and wildcard fields entirely,
        // so a combination that is invalid under `enabled` is inert under them.
        let config = CorsConfig {
            mode: CorsMode::Permissive,
            allow_credentials: true,
            allow_origins: OriginRule::Any,
            ..CorsConfig::default()
        };
        assert!(!config.has_credentialed_wildcard());
    }
}
