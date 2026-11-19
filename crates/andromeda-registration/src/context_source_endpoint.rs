//! The `endpoint` a Create or Update Context Source Registration writes.
//!
//! Table 5.2.9-1 makes `endpoint` the dereferenceable URI through which the
//! Context Source exposes its NGSI-LD interface, and clause 6.2 gives the root
//! of that interface the scheme `http` or `https`. A URI of any other scheme
//! names no NGSI-LD interface a forwarded request could reach, so the body
//! refuses it as it is read, as it refuses a URI that is not dereferenceable.
//!
//! The registration stores the [`DereferenceableUri`] alone, which takes any
//! scheme: the rule binds what a request writes, and every stored registration
//! loads whatever its endpoint.

use andromeda_ngsi_ld_lexicon::dereferenceable_uri::DereferenceableUri;
use derive_more::Into;
use serde::Deserialize;
use thiserror::Error;

/// A registration `endpoint` whose scheme is `http` or `https`.
#[derive(Debug, Deserialize, Into)]
#[serde(try_from = "DereferenceableUri")]
pub(crate) struct ContextSourceEndpoint(DereferenceableUri);

/// An `endpoint` whose scheme is neither `http` nor `https`.
#[derive(Debug, Error)]
#[error("endpoint must be an http or https URI, the schemes of an NGSI-LD API root, and {} is neither", .0.url().scheme())]
pub(crate) struct NotAnApiRoot(DereferenceableUri);

// Validating conversion: the scheme decides, which no derive checks.
impl TryFrom<DereferenceableUri> for ContextSourceEndpoint {
    type Error = NotAnApiRoot;

    fn try_from(endpoint: DereferenceableUri) -> Result<ContextSourceEndpoint, NotAnApiRoot> {
        match endpoint.url().scheme() {
            "http" | "https" => Ok(ContextSourceEndpoint(endpoint)),
            _other => Err(NotAnApiRoot(endpoint)),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::context_source_endpoint::{ContextSourceEndpoint, NotAnApiRoot};
    use andromeda_ngsi_ld_lexicon::dereferenceable_uri::DereferenceableUri;
    use pretty_assertions::assert_eq;

    fn uri(written: &str) -> DereferenceableUri {
        written.parse().expect("a dereferenceable URI")
    }

    /// The written spelling survives, as Table 5.2.9-1 has the broker serve
    /// the member back as it was supplied.
    #[test]
    fn an_http_or_https_endpoint_keeps_its_spelling() {
        for written in ["http://my.csource.org:1026", "https://my.csource.org/broker1", "HTTPS://My.CSource.org"] {
            let endpoint = ContextSourceEndpoint::try_from(uri(written)).expect("an NGSI-LD API root");
            assert_eq!(DereferenceableUri::from(endpoint).as_str(), written);
        }
    }

    /// Clause 6.2 gives an NGSI-LD API root the scheme `http` or `https`, so
    /// every other scheme is refused.
    #[test]
    fn an_endpoint_of_another_scheme_is_no_api_root() {
        for written in ["ftp://x/y", "file:///etc/passwd", "mqtt://broker:1883/topic", "urn:ngsi-ld:ContextSource:1"] {
            let refusal = ContextSourceEndpoint::try_from(uri(written));
            assert!(matches!(refusal, Err(NotAnApiRoot(ref endpoint)) if endpoint.as_str() == written), "{written}");
        }
    }

    /// The refusal is the `detail` a client reads, so it names the member, the
    /// schemes it takes and the one it was given.
    #[test]
    fn the_refusal_names_the_member_the_schemes_it_takes_and_the_one_given() {
        let refusal = ContextSourceEndpoint::try_from(uri("file:///etc/passwd")).expect_err("no NGSI-LD API root");
        assert_eq!(
            refusal.to_string(),
            "endpoint must be an http or https URI, the schemes of an NGSI-LD API root, and file is neither"
        );
    }

    /// The body reads the member through the same check, and a value that is
    /// no URI at all keeps the refusal of a URI that is not dereferenceable.
    #[test]
    fn the_member_is_read_through_the_check() {
        assert!(serde_json::from_str::<ContextSourceEndpoint>(r#""https://my.csource.org""#).is_ok());
        assert!(serde_json::from_str::<ContextSourceEndpoint>(r#""ftp://x/y""#).is_err());
        let refusal = serde_json::from_str::<ContextSourceEndpoint>(r#""nope""#).expect_err("no URI").to_string();
        assert!(refusal.starts_with("value is not a dereferenceable URI"), "{refusal}");
    }
}
