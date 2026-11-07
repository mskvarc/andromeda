//! Cross-Origin Resource Sharing policy for browser-facing clients.

use crate::human_duration::HumanDuration;
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{
    Deserialize,
    Deserializer,
    Serialize,
    Serializer,
    de::{Error as DeError, value::StrDeserializer},
};
use std::borrow::Cow;
use strum::{Display, EnumString};
use url::Url;

/// The browser-origin policy the broker installs at its edge.
///
/// `permissive` and `very-permissive` are complete profiles that ignore every
/// other `[http.cors]` field; `configured` builds the policy from the custom fields;
/// `disabled` installs no CORS middleware at all.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, Display, EnumString, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
#[schemars(description = "The CORS policy the broker applies to browser requests.")]
pub enum CorsMode {
    /// Install no CORS middleware.
    #[schemars(description = "Adds no CORS headers.")]
    Disabled,
    /// Build the CORS policy from the custom `[http.cors]` fields.
    #[schemars(
        description = "Builds the policy from `http.cors.allow_origins`, `http.cors.allow_methods`, `http.cors.allow_headers`, `http.cors.expose_headers`, `http.cors.credentials`, `http.cors.private_network` and `http.cors.preflight_lifetime`."
    )]
    Configured,
    /// Accept any origin, method and header, without credentials.
    #[default]
    #[schemars(description = "Allows any origin, method and header, without credentials. Every other `[http.cors]` key is ignored.")]
    Permissive,
    /// Mirror the request's origin, method and headers, and allow credentials.
    #[schemars(description = "Mirrors the request's origin, method and headers, and allows credentials. Every other `[http.cors]` key is ignored.")]
    VeryPermissive,
}

/// One HTTP method allowed through CORS, as the operator wrote its token.
///
/// The method grammar belongs to the HTTP layer, which converts the token
/// into its method type while the configuration loads, so a token that is no
/// method is refused there.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(transparent)]
#[schemars(description = "An HTTP method token, such as `\"GET\"`.")]
pub struct CorsMethod(String);

impl CorsMethod {
    /// The method token as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One HTTP header name allowed or exposed through CORS, as the operator
/// wrote it.
///
/// The field-name grammar belongs to the HTTP layer, which converts the name
/// into its header-name type while the configuration loads, so a name that is
/// no field name is refused there.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(transparent)]
#[schemars(description = "An HTTP header name, such as `\"Content-Type\"`.")]
pub struct CorsHeaderName(String);

impl CorsHeaderName {
    /// The header name as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
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
                $variant:ident = $token:literal => $description:literal
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
                        // An environment variable writes a list as its members
                        // separated by commas.
                        members => members
                            .split(',')
                            .map(str::trim)
                            .filter(|member| !member.is_empty())
                            .map(|member| <$elem>::deserialize(StrDeserializer::<D::Error>::new(member)).map_err(DeError::custom))
                            .collect::<Result<Vec<$elem>, D::Error>>()
                            .map($name::List),
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

        // Manual: a keyword or a list, a shape no derive states.
        impl JsonSchema for $name {
            fn schema_name() -> Cow<'static, str> {
                Cow::Borrowed(stringify!($name))
            }

            fn json_schema(generator: &mut SchemaGenerator) -> Schema {
                let list = generator.subschema_for::<Vec<$elem>>();
                json_schema!({
                    "anyOf": [
                        { "oneOf": [$({ "const": $token, "description": $description }),+] },
                        list
                    ]
                })
            }
        }
    };
}

cors_rule! {
    /// Which origins CORS accepts: any origin, or an explicit allow-list.
    enum OriginRule over Url {
        /// Accept requests from any origin (`Access-Control-Allow-Origin: *`).
        Any = "any" => "Accepts requests from any origin."
    }
}

cors_rule! {
    /// Which methods CORS allows: any, the request's own (mirrored), or a list.
    enum MethodRule over CorsMethod {
        /// Allow any method (`Access-Control-Allow-Methods: *`).
        Any = "any" => "Allows any method.",
        /// Reflect the method the request asks for.
        Mirror = "mirror" => "Allows the method the request asks for."
    }
}

cors_rule! {
    /// Which request headers CORS allows: any, the request's own, or a list.
    enum HeaderRule over CorsHeaderName {
        /// Allow any request header (`Access-Control-Allow-Headers: *`).
        Any = "any" => "Allows any request header.",
        /// Reflect the headers the request asks for.
        Mirror = "mirror" => "Allows the headers the request asks for."
    }
}

cors_rule! {
    /// Which response headers CORS exposes: any, or an explicit list.
    enum ExposeRule over CorsHeaderName {
        /// Expose any response header (`Access-Control-Expose-Headers: *`).
        Any = "any" => "Exposes any response header."
    }
}

/// Whether CORS grants a browser something it asks for.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, Display, EnumString, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
#[schemars(description = "Whether CORS grants a browser what it asks for.")]
pub enum CorsPermission {
    /// The browser is granted it.
    #[schemars(description = "Grants it.")]
    Allowed,
    /// The browser is refused it.
    #[default]
    #[schemars(description = "Refuses it.")]
    Refused,
}

/// CORS policy configuration for the broker edge.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[schemars(deny_unknown_fields)]
#[schemars(description = "The Cross-Origin Resource Sharing policy browser clients see.")]
pub struct CorsConfig {
    /// The overall CORS mode. `permissive` and `very-permissive` ignore every
    /// other field in this table.
    #[schemars(
        description = "Which CORS policy applies. Only `\"configured\"` reads the other keys of this table. `\"permissive\"` by default, so a browser-based interface works against the broker from any origin, and since it allows no credentials a page cannot act with a user's cookies; set `\"configured\"` to name the origins a deployment serves."
    )]
    pub mode: CorsMode,
    /// Origins accepted when `mode = configured`.
    #[schemars(
        description = "The origins allowed to read the API: `\"any\"`, or a list such as `[\"https://dashboard.example.org\"]`. Read only when `http.cors.mode` is `\"configured\"`."
    )]
    pub allow_origins: OriginRule,
    /// Methods allowed when `mode = configured`.
    #[schemars(
        description = "The methods allowed: `\"any\"`, `\"mirror\"` to allow whatever the preflight asks for, or a list such as `[\"GET\", \"POST\"]`. Read only when `http.cors.mode` is `\"configured\"`."
    )]
    pub allow_methods: MethodRule,
    /// Request headers allowed when `mode = configured`.
    #[schemars(
        description = "The request headers allowed: `\"any\"`, `\"mirror\"` to allow whatever the preflight asks for, or a list of header names. Read only when `http.cors.mode` is `\"configured\"`."
    )]
    pub allow_headers: HeaderRule,
    /// Response headers exposed to the browser when `mode = configured`.
    #[schemars(
        description = "The response headers a browser script may read: `\"any\"`, or a list of header names. Read only when `http.cors.mode` is `\"configured\"`."
    )]
    pub expose_headers: ExposeRule,
    /// Whether credentialed requests (cookies, HTTP auth) are `allowed` or
    /// `refused`. The Fetch standard forbids allowing them with an `any`
    /// wildcard, which the broker refuses at configuration time rather than
    /// letting the CORS layer panic.
    #[schemars(
        description = "Whether cookies and HTTP authentication may accompany a cross-origin request. Read only when `http.cors.mode` is `\"configured\"`."
    )]
    pub credentials: CorsPermission,
    /// Whether `Access-Control-Allow-Private-Network` is granted to a
    /// private-network preflight: `allowed` or `refused`.
    #[schemars(
        description = "Whether a private-network preflight is answered with `Access-Control-Allow-Private-Network`. Read only when `http.cors.mode` is `\"configured\"`."
    )]
    pub private_network: CorsPermission,
    /// How long a browser may cache a preflight result
    /// (`Access-Control-Max-Age`), in human-readable form (`"10m"`). Left out,
    /// the browser's own default applies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        description = "How long a browser may cache a preflight answer, sent as `Access-Control-Max-Age`. Left out, the broker sends no `Access-Control-Max-Age` and the browser's own default applies. Read only when `http.cors.mode` is `\"configured\"`."
    )]
    pub preflight_lifetime: Option<HumanDuration>,
}

impl Default for CorsConfig {
    fn default() -> CorsConfig {
        CorsConfig {
            mode: CorsMode::Permissive,
            allow_origins: OriginRule::Any,
            allow_methods: MethodRule::Any,
            allow_headers: HeaderRule::Any,
            expose_headers: ExposeRule::List(Vec::new()),
            credentials: CorsPermission::Refused,
            private_network: CorsPermission::Refused,
            preflight_lifetime: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::schema::http::cors::{CorsMode, ExposeRule, HeaderRule, MethodRule, OriginRule};

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
        for token in ["disabled", "configured", "permissive", "very-permissive"] {
            let holder: ModeHolder = toml::from_str(&format!("mode = \"{token}\"")).expect("mode parses");
            pretty_assertions::assert_eq!(holder.mode.to_string(), token);
        }
    }

    #[test]
    fn a_cors_mode_of_enabled_is_refused() {
        assert!(toml::from_str::<ModeHolder>("mode = \"enabled\"").is_err());
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
}
