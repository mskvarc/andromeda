//! Boundary deserialisers for the temporal-only query parameters.

use andromeda_ngsi_ld_lexicon::{aggr_method::AggrMethod, time_prop::TimeProp, time_rel::TimeRel};
use andromeda_ngsi_ld_query::comma_separated_list::{ListedItem, parse_listed};
use serde::{Deserialize, Deserializer, de::Error};

/// Deserialise the `timerel` query parameter as the [`TimeRel`] it names
/// (clause 4.11), rejecting an unknown relationship at the boundary.
///
/// # Errors
///
/// Returns the deserialiser's error when the value is present and not one of
/// `before`, `after`, `between`.
pub(crate) fn deserialize_option_timerel<'de, D>(deserializer: D) -> Result<Option<TimeRel>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw: Option<String> = Option::deserialize(deserializer)?;
    raw.map(|value| {
        value
            .parse::<TimeRel>()
            .map_err(|source| Error::custom(TemporalParameterError::Timerel(source)))
    })
    .transpose()
}

/// Deserialise the `timeproperty` query parameter as the [`TimeProp`] axis it
/// names (clause 4.8), rejecting an unknown property at the boundary.
///
/// # Errors
///
/// Returns the deserialiser's error when the value is present and not a defined
/// temporal property.
pub(crate) fn deserialize_option_timeproperty<'de, D>(deserializer: D) -> Result<Option<TimeProp>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw: Option<String> = Option::deserialize(deserializer)?;
    raw.map(|value| {
        value
            .parse::<TimeProp>()
            .map_err(|source| Error::custom(TemporalParameterError::TimeProperty(source)))
    })
    .transpose()
}

/// Why a temporal-only query parameter names nothing its clause defines.
#[derive(Debug, thiserror::Error)]
enum TemporalParameterError {
    /// `timerel` is not one of `before`, `after`, `between` (clause 4.11).
    #[error("invalid timerel: {0}")]
    Timerel(#[source] strum::ParseError),
    /// `timeproperty` names no temporal property (clause 4.8).
    #[error("invalid timeproperty: {0}")]
    TimeProperty(#[source] strum::ParseError),
}

/// Deserialize a comma-separated `aggrMethods` value into validated methods.
pub(crate) fn deserialize_optional_aggr_methods<'de, D>(deserializer: D) -> Result<Option<Vec<AggrMethod>>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw: Option<String> = Option::deserialize(deserializer)?;
    raw.map(|value| parse_listed(&value, ListedItem::AggregationMethod).map_err(Error::custom))
        .transpose()
}

#[cfg(test)]
mod tests {
    use crate::{params_from_query, query::temporal::TemporalQueryParameters, representation_parameter::WrittenValue};
    use andromeda_ngsi_ld_lexicon::{aggr_method::AggrMethod, temporal_format::TemporalFormat, time_prop::TimeProp, time_rel::TimeRel};

    fn params_from_query(query: &str) -> TemporalQueryParameters {
        params_from_query!(query, TemporalQueryParameters)
    }

    #[test]
    fn every_timerel_token_parses_to_its_relationship() {
        pretty_assertions::assert_eq!(params_from_query("timerel=before").timerel, Some(TimeRel::Before));
        pretty_assertions::assert_eq!(params_from_query("timerel=after").timerel, Some(TimeRel::After));
        pretty_assertions::assert_eq!(params_from_query("timerel=between").timerel, Some(TimeRel::Between));
    }

    #[test]
    fn timerel_invalid_is_rejected_at_the_boundary() {
        // Clause 4.11: an unknown `timerel` is a malformed request, rejected
        // while the query deserialises rather than by a later parse.
        let error = serde_urlencoded::from_str::<TemporalQueryParameters>("timerel=invalid").expect_err("an unknown timerel is refused");
        assert!(error.to_string().contains("invalid timerel"), "{error}");
    }

    #[test]
    fn timeproperty_created_at() {
        pretty_assertions::assert_eq!(params_from_query("timeproperty=createdAt").timeproperty, Some(TimeProp::Created));
    }

    #[test]
    fn timeproperty_rejects_unrecognized_value_at_the_boundary() {
        // Clause 4.8: `timeproperty` names the temporal axis. An unrecognised
        // value is a malformed request, not a silent fallback to observedAt.
        // A client's typo must not quietly query a different axis.
        let error = serde_urlencoded::from_str::<TemporalQueryParameters>("timeproperty=bogus").expect_err("an unknown timeproperty is refused");
        assert!(error.to_string().contains("invalid timeproperty"), "{error}");
    }

    #[test]
    fn the_two_temporal_formats_parse_and_an_unknown_one_is_rejected() {
        pretty_assertions::assert_eq!(
            params_from_query("format=temporalValues").format,
            Some(WrittenValue::Supported(TemporalFormat::TemporalValues))
        );
        pretty_assertions::assert_eq!(
            params_from_query("format=aggregatedValues").format,
            Some(WrittenValue::Supported(TemporalFormat::AggregatedValues))
        );
        // Clause 6.3.20: the value survives decoding so the refusal can name
        // its error type, and Table 6.3.12-1's grammar refuses it.
        let normalized: TemporalQueryParameters = serde_urlencoded::from_str("format=normalized").expect("the parameter set decodes");
        assert!(normalized.parse_format().is_err());
    }

    #[test]
    fn parse_aggr_methods_multiple() {
        let p = params_from_query("aggrMethods=avg,min,max");
        pretty_assertions::assert_eq!(p.aggr_methods, Some(vec![AggrMethod::Avg, AggrMethod::Min, AggrMethod::Max]));
    }

    #[test]
    fn parse_aggr_methods_empty_when_absent() {
        assert!(params_from_query("").aggr_methods.is_none());
    }

    #[test]
    fn an_unknown_aggr_method_is_rejected_at_the_boundary() {
        assert!(serde_urlencoded::from_str::<TemporalQueryParameters>("aggrMethods=median").is_err());
    }
}
