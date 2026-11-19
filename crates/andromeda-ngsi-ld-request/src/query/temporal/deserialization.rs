//! Boundary deserialisers for the temporal-only query parameters.

use andromeda_ngsi_ld_lexicon::aggr_method::AggrMethod;
use andromeda_ngsi_ld_query::{
    comma_separated_list::{CommaSeparatedListError, ListedMemberKind, listed_members},
    defined_token::{UndefinedToken, parse_token},
};
use serde::{Deserialize, Deserializer, de::Error};
use thiserror::Error;

/// Deserialize a comma-separated `aggrMethods` value into the clause 4.5.19
/// methods it names.
///
/// # Errors
///
/// Returns the deserialiser's error for an empty member, and for a member
/// that is no aggregation method, naming it and the methods defined.
pub(crate) fn deserialize_optional_aggr_methods<'de, D>(deserializer: D) -> Result<Option<Vec<AggrMethod>>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw: Option<String> = Option::deserialize(deserializer)?;
    raw.map(|value| aggregation_methods(&value).map_err(Error::custom)).transpose()
}

/// Why an `aggrMethods` list names no aggregation the clause defines.
#[derive(Debug, Error)]
enum AggrMethodsError {
    /// The list holds an empty member.
    #[error(transparent)]
    Empty(#[from] CommaSeparatedListError),
    /// A member is no aggregation method.
    #[error(transparent)]
    Undefined(#[from] UndefinedToken),
}

/// The aggregation methods the comma-separated list `raw` names.
fn aggregation_methods(raw: &str) -> Result<Vec<AggrMethod>, AggrMethodsError> {
    listed_members(raw, ListedMemberKind::AggregationMethod)
        .map(|member| Ok(parse_token(member?)?))
        .collect()
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

    /// Clause 4.11: an unknown `timerel` is a malformed request, refused
    /// while the query deserialises, naming the value and the relationships.
    #[test]
    fn timerel_invalid_is_rejected_at_the_boundary() {
        let error = serde_urlencoded::from_str::<TemporalQueryParameters>("timerel=invalid").expect_err("an unknown timerel is refused");
        pretty_assertions::assert_eq!(error.to_string(), "'invalid' is not one of before, after, between");
    }

    #[test]
    fn timeproperty_created_at() {
        pretty_assertions::assert_eq!(params_from_query("timeproperty=createdAt").timeproperty, Some(TimeProp::Created));
    }

    /// Clause 4.8: `timeproperty` names the temporal axis. An unrecognised
    /// value is a malformed request, not a silent fallback to observedAt: a
    /// client's typo must not quietly query a different axis.
    #[test]
    fn timeproperty_rejects_unrecognized_value_at_the_boundary() {
        let error = serde_urlencoded::from_str::<TemporalQueryParameters>("timeproperty=bogus").expect_err("an unknown timeproperty is refused");
        pretty_assertions::assert_eq!(error.to_string(), "'bogus' is not one of observedAt, createdAt, modifiedAt, deletedAt");
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
    fn an_unknown_aggr_method_is_rejected_at_the_boundary_naming_it_and_the_methods() {
        let error = serde_urlencoded::from_str::<TemporalQueryParameters>("aggrMethods=avg,median").expect_err("an unknown method is refused");
        pretty_assertions::assert_eq!(
            error.to_string(),
            "'median' is not one of avg, min, max, sum, sumsq, stddev, totalCount, distinctCount"
        );
        let empty = serde_urlencoded::from_str::<TemporalQueryParameters>("aggrMethods=avg,,min").expect_err("an empty member is refused");
        pretty_assertions::assert_eq!(empty.to_string(), "aggregation method list contains an empty member");
    }
}
