//! Typed `NGSILD-Scope` request header for Andromeda scope-query extensions.
//!
//! The header and `scopeQ` parameter have one grammar, owned by the query
//! crate. Keeping the parsed value here makes a header and URL request
//! indistinguishable after extraction.

use andromeda_ngsi_ld_query::scope::query::ScopeQuery;
use http::{HeaderName, HeaderValue};

/// The header name, as a `HeaderName` the `headers` trait can hand back.
static NGSILD_SCOPE: HeaderName = HeaderName::from_static("ngsild-scope");

/// Parsed `NGSILD-Scope` header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NgsiLdScope(ScopeQuery);

impl NgsiLdScope {
    /// The parsed scope query carried by this header.
    pub const fn query(&self) -> &ScopeQuery {
        &self.0
    }
}

impl headers::Header for NgsiLdScope {
    fn name() -> &'static HeaderName {
        &NGSILD_SCOPE
    }

    fn decode<'i, I>(values: &mut I) -> Result<NgsiLdScope, headers::Error>
    where
        I: Iterator<Item = &'i HeaderValue>,
    {
        let raw = values
            .next()
            .ok_or_else(headers::Error::invalid)?
            .to_str()
            .map_err(|_| headers::Error::invalid())?;
        raw.parse::<ScopeQuery>().map(NgsiLdScope).map_err(|_| headers::Error::invalid())
    }

    fn encode<E: Extend<HeaderValue>>(&self, values: &mut E) {
        match HeaderValue::from_str(&self.0.to_string()) {
            Ok(value) => values.extend(std::iter::once(value)),
            Err(error) => tracing::warn!(error = %error, "failed to encode NGSILD-Scope header"),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::ngsild_scope::NgsiLdScope;
    use andromeda_ngsi_ld_query::scope::query::ScopeQuery;
    use headers::Header;
    use http::HeaderValue;

    fn decode_from(raw: &str) -> Result<NgsiLdScope, headers::Error> {
        let value = HeaderValue::from_str(raw).expect("valid header value");
        let mut values = std::iter::once(&value);
        NgsiLdScope::decode(&mut values)
    }

    #[test]
    fn a_header_and_scope_query_parameter_decode_to_the_same_query() {
        let header = decode_from(" /A ; /B , /C/# ").expect("valid scope header");
        let parameter = "/A;/B,/C/#".parse::<ScopeQuery>().expect("valid scope query");
        pretty_assertions::assert_eq!(header.query(), &parameter);
    }

    #[test]
    fn malformed_scope_syntax_is_rejected_during_header_decoding() {
        assert!(decode_from("/A,,/B").is_err());
        assert!(decode_from("/A/#/B").is_err());
        assert!(decode_from("/1A/B").is_err());
    }

    #[test]
    fn encoding_uses_the_query_grammar_canonical_rendering() {
        let scope = decode_from("(/A|/B);/C").expect("valid scope header");
        let mut values = Vec::new();
        scope.encode(&mut values);
        pretty_assertions::assert_eq!(values, vec![HeaderValue::from_static("(/A;/C),(/B;/C)")]);
    }
}
