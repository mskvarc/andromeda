//! The upstream gate for the temporal aggregation grouping extension.
//!
//! `aggrGroupBy` and `format=groupedValues` are Class 1 additive features: with
//! the extension disabled a broker must behave exactly as one that never
//! implemented them, which clause 6.3.12's pattern makes an `InvalidRequest`
//! rather than a silently ignored parameter. The three aggregation-bearing
//! temporal query handlers — `GET /temporal/entities`, `GET
//! /temporal/entities/{id}` and `POST /temporal/entityOperations/query` — share
//! this gate, which runs before the request is admitted so a disabled broker
//! rejects the request before any of the grouping machinery is reached.

use andromeda_extensions::enabled::EnabledExtensions;
use andromeda_http_response::{classified_report::ClassifiedReport, problem_response::ProblemResponse};
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_ngsi_ld_request::query::temporal::TemporalQueryParameters;
use axum::http::Uri;

/// Reject a grouped temporal query when the extension is disabled.
///
/// A request asks for grouping when it carries `aggrGroupBy` or names the
/// grouped format through `format`/`options`. An invalid `format` value is left
/// for ordinary validation to report, so a value that does not even parse as the
/// grouped format is not treated as a grouping request here.
///
/// # Errors
///
/// Returns an `InvalidRequest` [`ProblemResponse`] when grouping is requested
/// while [`EnabledExtensions::temporal_aggregation_grouping_enabled`] is false.
pub(crate) fn reject_grouping_when_disabled(params: &TemporalQueryParameters, enabled: EnabledExtensions, uri: &Uri) -> Result<(), ProblemResponse> {
    if enabled.temporal_aggregation_grouping_enabled() {
        return Ok(());
    }
    let names_grouped_format = params.parse_format().is_ok_and(|format| format.is_grouped());
    if params.aggr_group_by.is_some() || names_grouped_format {
        let report = ClassifiedReport::public(
            NgsiLdErrorType::InvalidRequest,
            "aggrGroupBy and format=groupedValues require the temporal_aggregation_grouping extension".to_string(),
        );
        return Err(report.with_instance(uri.clone()));
    }
    Ok(())
}
