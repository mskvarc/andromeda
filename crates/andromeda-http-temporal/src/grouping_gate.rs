//! The upstream gate for the temporal aggregation grouping extension.
//!
//! `aggrGroupBy` and `format=groupedValues` are Class 1 additive features: with
//! the extension disabled a broker must behave exactly as one that never
//! implemented them, which clause 6.3.12's pattern makes an `InvalidRequest`
//! rather than a silently ignored parameter. The three aggregation-bearing
//! temporal query bindings -- `GET /temporal/entities`, `GET
//! /temporal/entities/{id}` and `POST /temporal/entityOperations/query` -- read
//! their parameters through the admitting extractors of
//! `andromeda_http_extraction::admitted_temporal_query`, which run this gate as
//! they read them, so a disabled broker rejects the request before any of the
//! grouping machinery is reached.

use andromeda_extensions::enabled::EnabledExtensions;
use andromeda_http_extraction::admitted_temporal_query::TemporalQueryAdmission;
use andromeda_http_response::classified_report::ClassifiedReport;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_ngsi_ld_request::query::temporal::TemporalQueryParameters;

/// The temporal routes' admission of a grouped query, by whether the grouping
/// extension is enabled.
#[derive(Debug, Clone, Copy)]
pub(crate) struct GroupingGate(EnabledExtensions);

impl GroupingGate {
    /// The gate for a broker with `enabled` extensions.
    pub(crate) const fn new(enabled: EnabledExtensions) -> GroupingGate {
        GroupingGate(enabled)
    }
}

impl TemporalQueryAdmission for GroupingGate {
    /// Refuse a grouped temporal query when the extension is disabled.
    ///
    /// A request asks for grouping when it carries `aggrGroupBy` or names the
    /// grouped format through `format`/`options`. An invalid `format` value is
    /// left for ordinary validation to report, so a value that does not even
    /// parse as the grouped format is not treated as a grouping request here.
    fn admit(&self, params: &TemporalQueryParameters) -> Result<(), ClassifiedReport> {
        if self.0.temporal_aggregation_grouping_enabled() {
            return Ok(());
        }
        let names_grouped_format = params.parse_format().is_ok_and(|format| format.is_grouped());
        if params.aggr_group_by.is_some() || names_grouped_format {
            return Err(ClassifiedReport::public(
                NgsiLdErrorType::InvalidRequest,
                "aggrGroupBy and format=groupedValues require the temporal_aggregation_grouping extension".to_string(),
            ));
        }
        Ok(())
    }
}
