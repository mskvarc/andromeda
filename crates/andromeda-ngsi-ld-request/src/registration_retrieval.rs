//! The parameters of Retrieve Context Source Registration (clause 5.10.1).
//!
//! Table 6.9.3.1-1 defines no URL parameter for the operation. Clause 6.3.11
//! gives every GET under `/csourceRegistrations/` the `options` parameter, and
//! the one registration is shaped by the clause 4.15 language filter as each
//! registration of a discovery is. Anything else, the discovery's selectors,
//! its paging and the clause 4.21 `pick` and `omit` included, is a parameter
//! the operation does not define: the clause 6.3.20 `InvalidRequest`.

use crate::{
    error::ParameterError,
    options::SysAttrsOptions,
    query_parameter::{QueryParameter, QueryParameterSet},
    query_parameter_set,
    representation_parameter::{WrittenValue, supported_option},
};
use andromeda_http_headers::language::LanguageFilter;
use serde::Deserialize;

/// Query parameters of Retrieve Context Source Registration.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistrationRetrieveParams {
    /// Clause 4.15 natural-language selection preferences (`lang`).
    #[serde(default)]
    pub lang: Option<LanguageFilter>,
    /// The system-attribute selection (clause 6.3.11 `options`).
    pub options: Option<WrittenValue<SysAttrsOptions>>,
}

query_parameter_set!(RegistrationRetrieveParams { Lang, Options });

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
mod tests {
    use crate::{
        query_parameter::{QueryParameter, QueryParameterSet},
        registration_retrieval::RegistrationRetrieveParams,
    };
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
    use pretty_assertions::assert_eq;

    #[test]
    fn options_and_lang_are_read() {
        let params: RegistrationRetrieveParams = serde_urlencoded::from_str("options=sysAttrs&lang=en").expect("parses");
        assert_eq!(params.parse_options().expect("a supported options value").sys_attrs, SysAttrsInclusion::Include);
        assert!(params.lang.is_some());
    }

    /// Table 6.9.3.1-1 defines no `pick` or `omit` for retrieval, and clause
    /// 5.2.9 makes `type`, `endpoint` and `information` mandatory, so each is
    /// a parameter the operation does not define (clause 6.3.20).
    #[test]
    fn pick_and_omit_are_unknown() {
        for query in ["pick=id", "omit=endpoint", "pick=id,endpoint&omit=type"] {
            let message = serde_urlencoded::from_str::<RegistrationRetrieveParams>(query)
                .expect_err("refused")
                .to_string();
            assert!(message.contains("unknown field"), "{query}: {message}");
        }
        assert!(!RegistrationRetrieveParams::defines(QueryParameter::Pick));
        assert!(!RegistrationRetrieveParams::defines(QueryParameter::Omit));
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
