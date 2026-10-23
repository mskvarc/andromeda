//! The upstream gate for the temporal query extensions.
//!
//! Two Class 1 additive features reach the temporal query bindings: grouped
//! aggregation (`aggrGroupBy`, `format=groupedValues`) and Linked Entity
//! retrieval (`join`, `joinLevel`, `containedBy`). With an extension disabled a
//! broker must behave exactly as one that never implemented it, which clause
//! 6.3.12's pattern makes an `InvalidRequest` rather than a silently ignored
//! parameter. The temporal query bindings -- `GET /temporal/entities`,
//! `GET /temporal/entities/{id}`, `POST /temporal/entityOperations/query` and
//! the temporal Entity Map creations over GET and POST -- read their parameters
//! through the admitting extractors of
//! `andromeda_http_extraction::admitted_temporal_query`, which run this gate as
//! they read them, so a disabled broker rejects the request before any of the
//! extension machinery is reached. The Entity Map routes install the same gate,
//! since clause 5.14.5.4 restates the temporal query they map.
//!
//! The `join` member of a clause 5.2.23 `Query` body is defined whatever the
//! broker enables, so the gate leaves it alone: Linked Entity admission refuses
//! a retrieving one there with `BadRequestData` (clause 5.7.4.4).

use andromeda_extensions::enabled::EnabledExtensions;
use andromeda_http_extraction::admitted_temporal_query::{TemporalQueryAdmission, TemporalQueryBinding};
use andromeda_http_response::classified_report::ClassifiedReport;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_ngsi_ld_request::query::temporal::TemporalQueryParameters;

/// The temporal routes' admission of an extended query, by which extensions
/// are enabled.
#[derive(Debug, Clone, Copy)]
pub struct TemporalExtensionGate(EnabledExtensions);

impl TemporalExtensionGate {
    /// The gate for a broker with `enabled` extensions.
    #[must_use]
    pub const fn new(enabled: EnabledExtensions) -> TemporalExtensionGate {
        TemporalExtensionGate(enabled)
    }

    /// Refuse a grouped temporal query when the grouping extension is disabled.
    ///
    /// A request asks for grouping when it carries `aggrGroupBy` or names the
    /// grouped format through `format`/`options`. An invalid `format` value is
    /// left for ordinary validation to report, so a value that does not even
    /// parse as the grouped format is not treated as a grouping request here.
    fn admit_grouping(self, params: &TemporalQueryParameters) -> Result<(), ClassifiedReport> {
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

    /// Refuse the join parameters on a temporal URL binding when temporal
    /// Linked Entity retrieval is disabled.
    ///
    /// The temporal binding tables define no `join`, `joinLevel` or
    /// `containedBy`; only Tables 6.4.3.2-1 and 6.5.3.1-1 do, so without the
    /// extension they are parameters the operation does not define.
    fn admit_linked_retrieval(self, params: &TemporalQueryParameters, binding: TemporalQueryBinding) -> Result<(), ClassifiedReport> {
        if self.0.temporal_linked_retrieval_enabled() {
            return Ok(());
        }
        let names_join_parameters = params.join.is_some() || params.join_level.is_some() || params.contained_by.is_some();
        match binding {
            TemporalQueryBinding::QueryString if names_join_parameters => Err(ClassifiedReport::public(
                NgsiLdErrorType::InvalidRequest,
                "join, joinLevel and containedBy are not defined for the temporal operations; they require the temporal_linked_retrieval extension".to_string(),
            )),
            TemporalQueryBinding::QueryString | TemporalQueryBinding::Body => Ok(()),
        }
    }
}

impl TemporalQueryAdmission for TemporalExtensionGate {
    /// Refuse a query that uses a disabled extension.
    fn admit(&self, params: &TemporalQueryParameters, binding: TemporalQueryBinding) -> Result<(), ClassifiedReport> {
        self.admit_grouping(params)?;
        self.admit_linked_retrieval(params, binding)
    }
}

#[cfg(test)]
mod tests {
    use crate::extension_gate::TemporalExtensionGate;
    use andromeda_extensions::enabled::EnabledExtensions;
    use andromeda_http_extraction::admitted_temporal_query::{TemporalQueryAdmission, TemporalQueryBinding};
    use andromeda_ngsi_ld_request::query::temporal::TemporalQueryParameters;

    fn params(raw: &str) -> TemporalQueryParameters {
        serde_urlencoded::from_str(raw).expect("valid temporal query")
    }

    /// With the extension off, each join parameter on a URL binding is one
    /// the temporal operations do not define.
    #[test]
    fn the_join_parameters_are_refused_on_a_url_binding_without_the_extension() {
        let gate = TemporalExtensionGate::new(EnabledExtensions::none());
        for raw in ["join=flat", "join=%40none", "joinLevel=2", "containedBy=urn:ngsi-ld:T:1"] {
            assert!(gate.admit(&params(raw), TemporalQueryBinding::QueryString).is_err(), "{raw}");
        }
        assert!(gate.admit(&params("type=T"), TemporalQueryBinding::QueryString).is_ok());
    }

    /// The `join` member of a query body is defined, so the gate leaves it to
    /// Linked Entity admission.
    #[test]
    fn a_body_join_is_left_to_admission() {
        let gate = TemporalExtensionGate::new(EnabledExtensions::none());
        assert!(gate.admit(&params("join=flat"), TemporalQueryBinding::Body).is_ok());
    }

    /// With the extension on, the join parameters pass on every binding.
    #[test]
    fn the_join_parameters_pass_with_the_extension() {
        let gate = TemporalExtensionGate::new(EnabledExtensions::all());
        let joined = params("join=flat&joinLevel=2&containedBy=urn:ngsi-ld:T:1");
        assert!(gate.admit(&joined, TemporalQueryBinding::QueryString).is_ok());
        assert!(gate.admit(&joined, TemporalQueryBinding::Body).is_ok());
    }

    /// Grouping is refused without its extension, on either binding.
    #[test]
    fn grouping_is_refused_without_its_extension() {
        let gate = TemporalExtensionGate::new(EnabledExtensions::none());
        let grouped = params("format=groupedValues&aggrMethods=avg&aggrGroupBy=unitCode");
        assert!(gate.admit(&grouped, TemporalQueryBinding::QueryString).is_err());
        assert!(gate.admit(&grouped, TemporalQueryBinding::Body).is_err());
    }
}
