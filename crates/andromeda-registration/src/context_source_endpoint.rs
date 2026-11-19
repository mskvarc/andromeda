//! The `endpoint` a Create or Update Context Source Registration writes.
//!
//! Table 5.2.9-1 makes `endpoint` the dereferenceable URI through which the
//! Context Source exposes its NGSI-LD interface, and clause 6.2 gives the root
//! of that interface the scheme `http` or `https`. A value that is no absolute
//! URI, or a URI of any other scheme, names no NGSI-LD interface a forwarded
//! request could reach, so the body refuses it as it is read, in the words of
//! that one rule.
//!
//! The registration stores the [`DereferenceableUri`] alone, which takes any
//! scheme: the rule binds what a request writes, and every stored registration
//! loads whatever its endpoint.

use andromeda_ngsi_ld_lexicon::dereferenceable_uri::DereferenceableUri;
use derive_more::Into;
use serde::Deserialize;
use thiserror::Error;

/// A registration `endpoint` that is an absolute `http` or `https` URI.
#[derive(Debug, Deserialize, Into)]
#[serde(try_from = "String")]
pub(crate) struct ContextSourceEndpoint(DereferenceableUri);

/// An `endpoint` that is no NGSI-LD API root.
#[derive(Debug, Error)]
pub(crate) enum NotAnApiRoot {
    /// The value is no absolute URI at all.
    #[error("endpoint must be an absolute http or https URI, the schemes of an NGSI-LD API root")]
    NotAnAbsoluteUri,
    /// The URI's scheme is neither `http` nor `https`.
    #[error(
        "endpoint must be an absolute http or https URI, the schemes of an NGSI-LD API root, and {} is neither",
        .0.url().scheme()
    )]
    OtherScheme(DereferenceableUri),
}

// Validating conversion: the URI grammar and the scheme decide, which no
// derive checks.
impl TryFrom<String> for ContextSourceEndpoint {
    type Error = NotAnApiRoot;

    fn try_from(written: String) -> Result<ContextSourceEndpoint, NotAnApiRoot> {
        let endpoint = written.parse::<DereferenceableUri>().map_err(|_not_a_uri| NotAnApiRoot::NotAnAbsoluteUri)?;
        match endpoint.url().scheme() {
            "http" | "https" => Ok(ContextSourceEndpoint(endpoint)),
            _other => Err(NotAnApiRoot::OtherScheme(endpoint)),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::context_source_endpoint::{ContextSourceEndpoint, NotAnApiRoot};
    use andromeda_ngsi_ld_lexicon::dereferenceable_uri::DereferenceableUri;
    use pretty_assertions::assert_eq;

    fn endpoint(written: &str) -> Result<ContextSourceEndpoint, NotAnApiRoot> {
        ContextSourceEndpoint::try_from(written.to_owned())
    }

    /// The written spelling survives, as Table 5.2.9-1 has the broker serve
    /// the member back as it was supplied.
    #[test]
    fn an_http_or_https_endpoint_keeps_its_spelling() {
        for written in ["http://my.csource.org:1026", "https://my.csource.org/broker1", "HTTPS://My.CSource.org"] {
            let endpoint = endpoint(written).expect("an NGSI-LD API root");
            assert_eq!(DereferenceableUri::from(endpoint).as_str(), written);
        }
    }

    /// Clause 6.2 gives an NGSI-LD API root the scheme `http` or `https`, so
    /// every other scheme is refused.
    #[test]
    fn an_endpoint_of_another_scheme_is_no_api_root() {
        for written in ["ftp://x/y", "file:///etc/passwd", "mqtt://broker:1883/topic", "urn:ngsi-ld:ContextSource:1"] {
            let refusal = endpoint(written);
            assert!(
                matches!(refusal, Err(NotAnApiRoot::OtherScheme(ref endpoint)) if endpoint.as_str() == written),
                "{written}"
            );
        }
    }

    /// A value that is no absolute URI names no scheme at all.
    #[test]
    fn a_value_that_is_no_absolute_uri_is_no_api_root() {
        for written in ["nope", "/ngsi-ld/v1", "", "http://"] {
            assert!(matches!(endpoint(written), Err(NotAnApiRoot::NotAnAbsoluteUri)), "{written}");
        }
    }

    /// The refusal is the `detail` a client reads, so it names the member, the
    /// URIs it takes and, when there is one, the scheme it was given.
    #[test]
    fn the_refusal_names_the_member_and_what_it_takes() {
        assert_eq!(
            endpoint("file:///etc/passwd").expect_err("no NGSI-LD API root").to_string(),
            "endpoint must be an absolute http or https URI, the schemes of an NGSI-LD API root, and file is neither"
        );
        assert_eq!(
            endpoint("nope").expect_err("no URI").to_string(),
            "endpoint must be an absolute http or https URI, the schemes of an NGSI-LD API root"
        );
    }

    #[test]
    fn the_member_is_read_through_the_check() {
        assert!(serde_json::from_str::<ContextSourceEndpoint>(r#""https://my.csource.org""#).is_ok());
        for refused in [r#""ftp://x/y""#, r#""nope""#] {
            let refusal = serde_json::from_str::<ContextSourceEndpoint>(refused).expect_err(refused).to_string();
            assert!(refusal.starts_with("endpoint must be an absolute http or https URI"), "{refusal}");
        }
    }
}
