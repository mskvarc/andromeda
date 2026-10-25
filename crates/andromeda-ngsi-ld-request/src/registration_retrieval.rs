//! The parameters of Retrieve Context Source Registration (clause 5.10.1).
//!
//! Table 6.9.3.1-1 defines no URL parameter for the operation. Clause 6.3.11
//! gives every GET under `/csourceRegistrations/` the `options` parameter, and
//! the one registration is shaped by the clause 4.21 projection and the clause
//! 4.15 language filter as each registration of a discovery is. Anything else,
//! the discovery's selectors and its paging included, is a parameter the
//! operation does not define: the clause 6.3.20 `InvalidRequest`.

use crate::{
    error::ParameterError,
    language_filter::deserialize_optional_language_filter,
    options::SysAttrsOptions,
    query::deserialization::deserialize_optional_projection_terms,
    representation_parameter::{WrittenValue, supported_option},
};
use andromeda_http_headers::language::LanguageFilter;
use andromeda_ngsi_ld_query::projection::grammar::RawProjectionTerm;
use serde::Deserialize;

/// Query parameters of Retrieve Context Source Registration.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistrationRetrieveParams {
    /// Members the registration is narrowed to (clause 4.21 `pick`).
    #[serde(default, deserialize_with = "deserialize_optional_projection_terms")]
    pub pick: Option<Vec<RawProjectionTerm>>,
    /// Members the registration drops (clause 4.21 `omit`).
    #[serde(default, deserialize_with = "deserialize_optional_projection_terms")]
    pub omit: Option<Vec<RawProjectionTerm>>,
    /// Clause 4.15 natural-language selection preferences (`lang`).
    #[serde(default, deserialize_with = "deserialize_optional_language_filter")]
    pub lang: Option<LanguageFilter>,
    /// The system-attribute selection (clause 6.3.11 `options`).
    pub options: Option<WrittenValue<SysAttrsOptions>>,
}

impl RegistrationRetrieveParams {
    /// The system-attribute selection the `options` parameter made.
    ///
    /// # Errors
    ///
    /// Returns [`ParameterError::OptionsInvalid`] for a value clause 6.3.11
    /// does not define (clause 6.3.20).
    pub fn parse_options(&self) -> Result<SysAttrsOptions, ParameterError> {
        Ok(supported_option(self.options.as_ref())?.unwrap_or_default())
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::registration_retrieval::RegistrationRetrieveParams;
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
    use pretty_assertions::assert_eq;

    #[test]
    fn options_pick_omit_and_lang_are_read() {
        let params: RegistrationRetrieveParams = serde_urlencoded::from_str("options=sysAttrs&pick=id,endpoint&omit=type&lang=en").expect("parses");
        assert_eq!(params.parse_options().expect("a supported options value").sys_attrs, SysAttrsInclusion::Include);
        assert_eq!(params.pick.as_deref().map(<[_]>::len), Some(2));
        assert_eq!(params.omit.as_deref().map(<[_]>::len), Some(1));
        assert!(params.lang.is_some());
    }

    /// Table 6.9.3.1-1 defines no discovery selector and no paging, so each
    /// is a parameter the operation does not define (clause 6.3.20).
    #[test]
    fn every_discovery_parameter_is_unknown() {
        for query in [
            "type=Vehicle",
            "id=urn:ngsi-ld:Vehicle:1",
            "idPattern=urn:.*",
            "attrs=speed",
            "q=speed>1",
            "csf=tier==1",
            "scopeQ=/Madrid",
            "datasetId=@none",
            "geometryProperty=location",
            "georel=near%3BmaxDistance==1&geometry=Point&coordinates=[1,2]",
            "timerel=before&timeAt=2024-01-01T00:00:00Z",
            "timeproperty=observedAt",
            "limit=3",
            "offset=1",
            "cursor=abc",
        ] {
            let message = serde_urlencoded::from_str::<RegistrationRetrieveParams>(query)
                .expect_err("refused")
                .to_string();
            assert!(message.contains("unknown field"), "{query}: {message}");
        }
    }

    /// `count` belongs to a listing (clause 6.3.13), not to one registration.
    #[test]
    fn options_count_is_refused() {
        let params: RegistrationRetrieveParams = serde_urlencoded::from_str("options=count").expect("parses");
        assert!(params.parse_options().is_err());
    }
}
