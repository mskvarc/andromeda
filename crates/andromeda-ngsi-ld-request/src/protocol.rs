//! Merging the clause 6.3.x protocol extension headers into a query.
//!
//! Andromeda accepts several query parameters as headers as well, so a client
//! that cannot put a long `scopeQ` or a `format` in a URL can send it another
//! way. The rule is the same for all of them: the query parameter wins, and the
//! header fills in only what the query left unsaid.
//!
//! The merge takes the *typed* header values a validated `NGSILD-*` header
//! decoded to, not their text, so a scope, a format token or a dataset id set
//! is not re-parsed here after the header crate already parsed it. Deciding
//! what a request means once the values are in hand is this crate's job, and it
//! serves a server, a test and a forwarded request alike.

use crate::{
    entity::GetEntityParams,
    error::ParameterError,
    query::{entity::parameters::EntityQueryParameters, temporal::TemporalQueryParameters},
};
use andromeda_ngsi_ld_http_headers::{ngsild_dataset_id::NgsiLdDatasetId, ngsild_response_format::NgsiLdResponseFormat, ngsild_scope::NgsiLdScope};
use andromeda_ngsi_ld_lexicon::temporal_format::TemporalFormat;
use andromeda_ngsi_ld_representation::{join_mode::JoinMode, response_format::ResponseFormat};
use std::{num::NonZeroU32, str::FromStr};

/// The extension headers a request carried, as the typed values they decoded to.
///
/// Every field is the already-parsed value of the corresponding `NGSILD-*`
/// header, or `None` if it was absent. [`Default`] is the no-headers case.
#[derive(Debug, Default)]
pub struct ProtocolExtensions<'a> {
    /// `NGSILD-Scope`, carrying the `scopeQ` expression it validated.
    pub scope: Option<&'a NgsiLdScope>,
    /// `NGSILD-Response-Format`, as the representation it decoded to.
    pub response_format: Option<NgsiLdResponseFormat>,
    /// `NGSILD-Dataset-Id`, carrying the dataset ids it validated.
    pub dataset_ids: Option<&'a NgsiLdDatasetId>,
    /// `NGSILD-Join`.
    pub join: Option<JoinMode>,
    /// `NGSILD-Join-Level`.
    pub join_level: Option<NonZeroU32>,
}

/// Whether a query parameter's `format`/`options` token names an entity
/// representation (clause 4.5.1), decided by the owner enum rather than a
/// hand-kept string array.
fn is_entity_format(token: &str) -> bool {
    ResponseFormat::from_str(token).is_ok()
}

/// Whether a decoded `NGSILD-Response-Format` names a temporal representation
/// (clause 4.5.19). The plain `normalized` is the default, not a temporal
/// override, so only the two value-oriented tokens count.
const fn header_is_temporal_format(format: NgsiLdResponseFormat) -> bool {
    matches!(format, NgsiLdResponseFormat::TemporalValues | NgsiLdResponseFormat::AggregatedValues)
}

impl<'a> ProtocolExtensions<'a> {
    /// Fill in a single-entity GET from the headers it arrived with.
    ///
    /// # Errors
    ///
    /// Returns [`ParameterError::FormatInvalid`] when the header names a
    /// rendering that is not one an entity has.
    pub fn apply_to_entity_get(&self, params: &mut GetEntityParams) -> Result<(), ParameterError> {
        self.apply_format(&mut params.format, params.options.as_deref())?;
        self.apply_dataset_ids(&mut params.dataset_id);
        self.apply_join(&mut params.join, &mut params.join_level);
        Ok(())
    }

    /// Fill in an entity query from the headers it arrived with.
    ///
    /// # Errors
    ///
    /// Returns [`ParameterError::FormatInvalid`] when the header names a
    /// rendering that is not one an entity has.
    pub fn apply_to_entity_query(&self, params: &mut EntityQueryParameters) -> Result<(), ParameterError> {
        if params.scope_q.is_none()
            && let Some(scope) = self.scope
        {
            // The header is borrowed by the extractor; the request outlives
            // that borrow, so owning its already-validated AST is necessary.
            params.scope_q = Some(scope.query().clone());
        }
        self.apply_format(&mut params.format, params.options.as_deref())?;
        self.apply_dataset_ids(&mut params.dataset_id);
        self.apply_join(&mut params.join, &mut params.join_level);
        Ok(())
    }

    /// Fill in a temporal query from the headers it arrived with.
    ///
    /// # Errors
    ///
    /// Returns [`ParameterError::FormatInvalid`] when the header names a
    /// rendering that is not one a temporal representation has.
    pub fn apply_to_temporal_query(&self, params: &mut TemporalQueryParameters) -> Result<(), ParameterError> {
        if params.format.is_none()
            && let Some(response_format) = self.response_format
        {
            if !header_is_temporal_format(response_format) {
                return Err(ParameterError::FormatInvalid(<&'static str>::from(response_format).to_string()));
            }
            params.format = Some(match response_format {
                NgsiLdResponseFormat::TemporalValues => TemporalFormat::TemporalValues,
                NgsiLdResponseFormat::AggregatedValues => TemporalFormat::AggregatedValues,
                NgsiLdResponseFormat::Normalized | NgsiLdResponseFormat::KeyValues | NgsiLdResponseFormat::Simplified | NgsiLdResponseFormat::Concise => {
                    return Err(ParameterError::FormatInvalid(<&'static str>::from(response_format).to_string()));
                }
            });
        }
        self.apply_dataset_ids(&mut params.dataset_id);
        Ok(())
    }

    /// Apply `NGSILD-Response-Format`, unless the query already chose a rendering.
    ///
    /// `format` and the legacy `options` are two spellings of the same choice,
    /// so a rendering named in either one silences the header.
    fn apply_format(&self, format: &mut Option<ResponseFormat>, options: Option<&str>) -> Result<(), ParameterError> {
        let query_chose_a_format = format.is_some() || options.is_some_and(|options| options.split(',').any(|token| is_entity_format(token.trim())));
        if query_chose_a_format {
            return Ok(());
        }

        let Some(response_format) = self.response_format else {
            return Ok(());
        };
        // An entity rendering is any of the four that are not temporal.
        if header_is_temporal_format(response_format) {
            return Err(ParameterError::FormatInvalid(<&'static str>::from(response_format).to_string()));
        }
        *format = Some(match response_format {
            NgsiLdResponseFormat::Normalized => ResponseFormat::Normalized,
            NgsiLdResponseFormat::KeyValues | NgsiLdResponseFormat::Simplified => ResponseFormat::Simplified,
            NgsiLdResponseFormat::Concise => ResponseFormat::Concise,
            NgsiLdResponseFormat::TemporalValues | NgsiLdResponseFormat::AggregatedValues => {
                return Err(ParameterError::FormatInvalid(<&'static str>::from(response_format).to_string()));
            }
        });
        Ok(())
    }

    /// Apply `NGSILD-Dataset-Id`, unless the query already selected datasets.
    ///
    /// The header carried the ids as validated URNs; they are joined into the
    /// comma-separated form the `datasetId` parameter takes only here, once.
    fn apply_dataset_ids(&self, target: &mut Option<Vec<urn_rs::Urn>>) {
        if target.is_none()
            && let Some(dataset_ids) = self.dataset_ids
        {
            *target = Some(dataset_ids.as_slice().to_vec());
        }
    }

    /// Apply `NGSILD-Join` and `NGSILD-Join-Level`, each independently.
    const fn apply_join(&self, join: &mut Option<JoinMode>, join_level: &mut Option<NonZeroU32>) {
        if join.is_none()
            && let Some(mode) = self.join
        {
            *join = Some(mode);
        }
        if join_level.is_none()
            && let Some(level) = self.join_level
        {
            *join_level = Some(level);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        entity::GetEntityParams,
        error::ParameterError,
        protocol::ProtocolExtensions,
        query::{entity::parameters::EntityQueryParameters, temporal::TemporalQueryParameters},
    };
    use andromeda_ngsi_ld_http_headers::{ngsild_dataset_id::NgsiLdDatasetId, ngsild_response_format::NgsiLdResponseFormat, ngsild_scope::NgsiLdScope};
    use andromeda_ngsi_ld_lexicon::temporal_format::TemporalFormat;
    use andromeda_ngsi_ld_representation::{join_mode::JoinMode, response_format::ResponseFormat};
    use headers::Header;
    use http::HeaderValue;
    use std::num::NonZeroU32;

    fn get_params() -> GetEntityParams {
        serde_urlencoded::from_str("").expect("empty query parses")
    }

    /// Decode an `NGSILD-Scope` header the way the extractor does, so the merge
    /// is exercised against a value the header crate actually validated.
    fn scope(raw: &str) -> NgsiLdScope {
        let value = HeaderValue::from_str(raw).expect("valid header value");
        NgsiLdScope::decode(&mut std::iter::once(&value)).expect("valid scope header")
    }

    /// Decode an `NGSILD-Dataset-Id` header, as the extractor does.
    fn dataset_ids(raw: &str) -> NgsiLdDatasetId {
        let value = HeaderValue::from_str(raw).expect("valid header value");
        NgsiLdDatasetId::decode(&mut std::iter::once(&value)).expect("valid dataset id header")
    }

    #[test]
    fn absent_headers_change_nothing() {
        let mut params = get_params();
        ProtocolExtensions::default().apply_to_entity_get(&mut params).expect("no headers");
        pretty_assertions::assert_eq!(params.format, None);
        pretty_assertions::assert_eq!(params.dataset_id, None);
        pretty_assertions::assert_eq!(params.join, None);
        pretty_assertions::assert_eq!(params.join_level, None);
    }

    #[test]
    fn a_header_fills_in_only_what_the_query_left_unsaid() {
        let mut params: GetEntityParams = serde_urlencoded::from_str("format=keyValues&datasetId=urn:ds:1&join=inline&joinLevel=2").expect("query parses");
        let ds = dataset_ids("urn:ds:9");
        let extensions = ProtocolExtensions {
            scope: None,
            response_format: Some(NgsiLdResponseFormat::Concise),
            dataset_ids: Some(&ds),
            join: Some(JoinMode::Flat),
            join_level: NonZeroU32::new(7),
        };
        extensions.apply_to_entity_get(&mut params).expect("headers apply");

        pretty_assertions::assert_eq!(params.format, Some(ResponseFormat::Simplified));
        pretty_assertions::assert_eq!(params.dataset_id.as_deref().map(|ids| ids[0].as_str()), Some("urn:ds:1"));
        pretty_assertions::assert_eq!(params.join, Some(JoinMode::Inline));
        pretty_assertions::assert_eq!(params.join_level.map(NonZeroU32::get), Some(2));
    }

    #[test]
    fn a_header_is_applied_when_the_query_said_nothing() {
        let mut params = get_params();
        let ds = dataset_ids("urn:ds:9");
        let extensions = ProtocolExtensions {
            scope: None,
            response_format: Some(NgsiLdResponseFormat::Concise),
            dataset_ids: Some(&ds),
            join: Some(JoinMode::Flat),
            join_level: NonZeroU32::new(7),
        };
        extensions.apply_to_entity_get(&mut params).expect("headers apply");

        pretty_assertions::assert_eq!(params.format, Some(ResponseFormat::Concise));
        pretty_assertions::assert_eq!(params.dataset_id.as_deref().map(|ids| ids[0].as_str()), Some("urn:ds:9"));
        pretty_assertions::assert_eq!(params.join, Some(JoinMode::Flat));
        pretty_assertions::assert_eq!(params.join_level.map(NonZeroU32::get), Some(7));
    }

    #[test]
    fn a_rendering_named_in_the_legacy_options_parameter_also_silences_the_header() {
        let mut params: GetEntityParams = serde_urlencoded::from_str("options=keyValues").expect("query parses");
        let extensions = ProtocolExtensions {
            response_format: Some(NgsiLdResponseFormat::Concise),
            ..ProtocolExtensions::default()
        };
        extensions.apply_to_entity_get(&mut params).expect("headers apply");
        pretty_assertions::assert_eq!(params.format, None);
    }

    #[test]
    fn a_scope_header_fills_in_an_absent_scope_query() {
        let mut params: EntityQueryParameters = serde_urlencoded::from_str("type=Vehicle").expect("query parses");
        let scope_header = scope("/A;/B");
        let extensions = ProtocolExtensions {
            scope: Some(&scope_header),
            ..ProtocolExtensions::default()
        };
        extensions.apply_to_entity_query(&mut params).expect("headers apply");
        pretty_assertions::assert_eq!(params.scope_q, Some("/A;/B".parse().expect("valid scope query")));
    }

    #[test]
    fn a_temporal_query_takes_only_a_temporal_rendering() {
        let mut params: TemporalQueryParameters = serde_urlencoded::from_str("").expect("query parses");
        let extensions = ProtocolExtensions {
            response_format: Some(NgsiLdResponseFormat::TemporalValues),
            ..ProtocolExtensions::default()
        };
        extensions.apply_to_temporal_query(&mut params).expect("headers apply");
        pretty_assertions::assert_eq!(params.format, Some(TemporalFormat::TemporalValues));
    }

    #[test]
    fn a_rendering_the_resource_does_not_have_is_rejected() {
        let mut entity = get_params();
        let entity_error = ProtocolExtensions {
            response_format: Some(NgsiLdResponseFormat::TemporalValues),
            ..ProtocolExtensions::default()
        }
        .apply_to_entity_get(&mut entity)
        .expect_err("temporalValues is not an entity rendering");
        assert!(matches!(entity_error, ParameterError::FormatInvalid(format) if format == "temporalValues"));

        let mut temporal: TemporalQueryParameters = serde_urlencoded::from_str("").expect("query parses");
        let temporal_error = ProtocolExtensions {
            response_format: Some(NgsiLdResponseFormat::Concise),
            ..ProtocolExtensions::default()
        }
        .apply_to_temporal_query(&mut temporal)
        .expect_err("concise is not a temporal rendering");
        assert!(matches!(temporal_error, ParameterError::FormatInvalid(format) if format == "concise"));
    }
}
