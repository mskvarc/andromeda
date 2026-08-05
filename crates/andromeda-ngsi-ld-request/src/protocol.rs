//! Merging the clause 6.3.x protocol extension headers into a query.
//!
//! Andromeda accepts several query parameters as headers as well, so a client
//! that cannot put a long `scopeQ` or a `format` in a URL can send it another
//! way. The rule is the same for all of them: the query parameter wins, and the
//! header fills in only what the query left unsaid.
//!
//! The merge takes header *values*, not header types. Extracting a header is
//! the web framework's job; deciding what a request means once the values are
//! in hand is this crate's, and keeping the split there is what lets the same
//! merge serve a server, a test, and a forwarded request alike.

use crate::{
    entity::GetEntityParams,
    error::ParameterError,
    query::{entity::EntityQueryParameters, temporal::TemporalQueryParameters},
};
use andromeda_ngsi_ld_representation::join_mode::JoinMode;

/// The extension headers a request carried, as the values they spell.
///
/// Every field is what the corresponding `NGSILD-*` header decoded to, or
/// `None` if it was absent. [`Default`] is the no-headers case.
#[derive(Debug, Default)]
pub struct ProtocolExtensions<'a> {
    /// `NGSILD-Scope`, as the `scopeQ` expression it carries.
    pub scope: Option<&'a str>,
    /// `NGSILD-Response-Format`, as the token the `format` parameter takes.
    pub response_format: Option<&'a str>,
    /// `NGSILD-Dataset-Id`, already joined into the comma-separated form the
    /// `datasetId` parameter takes.
    pub dataset_ids: Option<String>,
    /// `NGSILD-Join`.
    pub join: Option<JoinMode>,
    /// `NGSILD-Join-Level`.
    pub join_level: Option<u32>,
}

/// The `format` tokens that name an entity representation (clause 4.5.1).
const ENTITY_FORMATS: [&str; 4] = ["normalized", "keyValues", "simplified", "concise"];

/// The `format` tokens that name a temporal representation (clause 4.5.19).
const TEMPORAL_FORMATS: [&str; 2] = ["temporalValues", "aggregatedValues"];

impl<'a> ProtocolExtensions<'a> {
    /// Fill in a single-entity GET from the headers it arrived with.
    ///
    /// # Errors
    ///
    /// Returns [`ParameterError::FormatInvalid`] when the header names a
    /// rendering that is not one an entity has.
    pub fn apply_to_entity_get(&self, params: &mut GetEntityParams) -> Result<(), ParameterError> {
        self.apply_format(&mut params.format, params.options.as_deref(), &ENTITY_FORMATS)?;
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
            params.scope_q = Some(scope.to_string());
        }
        self.apply_format(&mut params.format, params.options.as_deref(), &ENTITY_FORMATS)?;
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
            if !TEMPORAL_FORMATS.contains(&response_format) {
                return Err(ParameterError::FormatInvalid(response_format.to_string()));
            }
            params.format = Some(response_format.to_string());
        }
        self.apply_dataset_ids(&mut params.dataset_id);
        Ok(())
    }

    /// Apply `NGSILD-Response-Format`, unless the query already chose a rendering.
    ///
    /// `format` and the legacy `options` are two spellings of the same choice,
    /// so a rendering named in either one silences the header.
    fn apply_format(&self, format: &mut Option<String>, options: Option<&str>, permitted: &[&str]) -> Result<(), ParameterError> {
        let query_chose_a_format = format.as_deref().is_some_and(|format| permitted.contains(&format))
            || options.is_some_and(|options| permitted.iter().any(|permitted| options.contains(permitted)));
        if query_chose_a_format {
            return Ok(());
        }

        let Some(response_format) = self.response_format else {
            return Ok(());
        };
        if !permitted.contains(&response_format) {
            return Err(ParameterError::FormatInvalid(response_format.to_string()));
        }
        *format = Some(response_format.to_string());
        Ok(())
    }

    /// Apply `NGSILD-Dataset-Id`, unless the query already selected datasets.
    fn apply_dataset_ids(&self, target: &mut Option<String>) {
        if target.is_none() {
            target.clone_from(&self.dataset_ids);
        }
    }

    /// Apply `NGSILD-Join` and `NGSILD-Join-Level`, each independently.
    fn apply_join(&self, join: &mut Option<String>, join_level: &mut Option<u32>) {
        if join.is_none()
            && let Some(mode) = self.join
        {
            *join = Some(mode.to_string());
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
        query::{entity::EntityQueryParameters, temporal::TemporalQueryParameters},
    };
    use andromeda_ngsi_ld_representation::join_mode::JoinMode;

    fn get_params() -> GetEntityParams {
        serde_urlencoded::from_str("").expect("empty query parses")
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
        let extensions = ProtocolExtensions {
            scope: None,
            response_format: Some("concise"),
            dataset_ids: Some("urn:ds:9".to_string()),
            join: Some(JoinMode::Flat),
            join_level: Some(7),
        };
        extensions.apply_to_entity_get(&mut params).expect("headers apply");

        pretty_assertions::assert_eq!(params.format.as_deref(), Some("keyValues"));
        pretty_assertions::assert_eq!(params.dataset_id.as_deref(), Some("urn:ds:1"));
        pretty_assertions::assert_eq!(params.join.as_deref(), Some("inline"));
        pretty_assertions::assert_eq!(params.join_level, Some(2));
    }

    #[test]
    fn a_header_is_applied_when_the_query_said_nothing() {
        let mut params = get_params();
        let extensions = ProtocolExtensions {
            scope: None,
            response_format: Some("concise"),
            dataset_ids: Some("urn:ds:9".to_string()),
            join: Some(JoinMode::Flat),
            join_level: Some(7),
        };
        extensions.apply_to_entity_get(&mut params).expect("headers apply");

        pretty_assertions::assert_eq!(params.format.as_deref(), Some("concise"));
        pretty_assertions::assert_eq!(params.dataset_id.as_deref(), Some("urn:ds:9"));
        pretty_assertions::assert_eq!(params.join.as_deref(), Some("flat"));
        pretty_assertions::assert_eq!(params.join_level, Some(7));
    }

    #[test]
    fn a_rendering_named_in_the_legacy_options_parameter_also_silences_the_header() {
        let mut params: GetEntityParams = serde_urlencoded::from_str("options=keyValues").expect("query parses");
        let extensions = ProtocolExtensions {
            response_format: Some("concise"),
            ..ProtocolExtensions::default()
        };
        extensions.apply_to_entity_get(&mut params).expect("headers apply");
        pretty_assertions::assert_eq!(params.format, None);
    }

    #[test]
    fn a_scope_header_fills_in_an_absent_scope_query() {
        let mut params: EntityQueryParameters = serde_urlencoded::from_str("type=Vehicle").expect("query parses");
        let extensions = ProtocolExtensions {
            scope: Some("/A;/B"),
            ..ProtocolExtensions::default()
        };
        extensions.apply_to_entity_query(&mut params).expect("headers apply");
        pretty_assertions::assert_eq!(params.scope_q.as_deref(), Some("/A;/B"));
    }

    #[test]
    fn a_temporal_query_takes_only_a_temporal_rendering() {
        let mut params: TemporalQueryParameters = serde_urlencoded::from_str("").expect("query parses");
        let extensions = ProtocolExtensions {
            response_format: Some("temporalValues"),
            ..ProtocolExtensions::default()
        };
        extensions.apply_to_temporal_query(&mut params).expect("headers apply");
        pretty_assertions::assert_eq!(params.format.as_deref(), Some("temporalValues"));
    }

    #[test]
    fn a_rendering_the_resource_does_not_have_is_rejected() {
        let mut entity = get_params();
        let entity_error = ProtocolExtensions {
            response_format: Some("temporalValues"),
            ..ProtocolExtensions::default()
        }
        .apply_to_entity_get(&mut entity)
        .expect_err("temporalValues is not an entity rendering");
        assert!(matches!(entity_error, ParameterError::FormatInvalid(format) if format == "temporalValues"));

        let mut temporal: TemporalQueryParameters = serde_urlencoded::from_str("").expect("query parses");
        let temporal_error = ProtocolExtensions {
            response_format: Some("concise"),
            ..ProtocolExtensions::default()
        }
        .apply_to_temporal_query(&mut temporal)
        .expect_err("concise is not a temporal rendering");
        assert!(matches!(temporal_error, ParameterError::FormatInvalid(format) if format == "concise"));
    }
}
