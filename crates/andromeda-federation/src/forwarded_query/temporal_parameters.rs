//! The temporal parameters a forwarded temporal query carries (clauses 5.7.3.4,
//! 5.7.4.4, 6.3.12).

use crate::{
    forwarded_parameter::ForwardedParameter,
    forwarded_query::{ForwardedQuery, SYS_ATTRS_OPTION},
};
use andromeda_ngsi_ld_lexicon::{rfc3339::sub_second, temporal_format::TemporalFormat};
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use andromeda_ngsi_ld_request::{query::temporal::TemporalQueryParameters, representation_parameter::WrittenValue};

impl ForwardedQuery {
    /// The temporal window, representation and aggregation parameters a temporal
    /// query forwards (clauses 5.7.3.4, 5.7.4.4, 6.3.12), and its clause 4.5.23
    /// traversal continuation.
    pub(super) fn push_temporal_parameters(&mut self, params: &TemporalQueryParameters) {
        self.push_optional(ForwardedParameter::Timerel, params.timerel.map(|timerel| timerel.to_string()));
        self.push_optional(ForwardedParameter::TimeAt, params.time_at.map(sub_second));
        self.push_optional(ForwardedParameter::EndTimeAt, params.end_time_at.map(sub_second));
        self.push_optional(ForwardedParameter::Timeproperty, params.timeproperty.map(|property| property.to_string()));
        self.push_optional(ForwardedParameter::LastN, params.last_n.map(|last_n| last_n.to_string()));
        // Clause 6.3.12: `normalized` is the absence of a temporal `format`, not
        // a value a source accepts. The grouped extension format is never
        // forwarded (study §4.10): grouping is applied by this broker after the
        // merge, so a source is asked for the plain aggregated representation and
        // `aggrGroupBy` stays local.
        self.push_optional(
            ForwardedParameter::Format,
            params
                .format
                .as_ref()
                .and_then(WrittenValue::if_supported)
                .filter(|format| **format != TemporalFormat::Normalized)
                .map(|format| match format {
                    TemporalFormat::GroupedValues => TemporalFormat::AggregatedValues.to_string(),
                    TemporalFormat::Normalized | TemporalFormat::TemporalValues | TemporalFormat::AggregatedValues => format.to_string(),
                }),
        );
        self.push_temporal_options(params);
        self.push_optional(
            ForwardedParameter::AggrMethods,
            params
                .aggr_methods
                .as_deref()
                .filter(|methods| !methods.is_empty())
                .map(|methods| methods.iter().map(ToString::to_string).collect::<Vec<_>>().join(",")),
        );
        self.push_optional(
            ForwardedParameter::AggrPeriodDuration,
            params.aggr_period_duration.map(|period| period.as_iso8601()),
        );
        self.push_traversal(params.join.map(|join| join.to_string()), params.join_level, params.contained_by.as_deref());
    }

    /// The temporal `options` tokens: the representation a source answers in
    /// has to be the one the local history is rendered in for the two to merge,
    /// so the deprecated `options` spelling of the format travels as written.
    fn push_temporal_options(&mut self, params: &TemporalQueryParameters) {
        let Some(options) = params.options.as_ref().and_then(WrittenValue::if_supported) else {
            return;
        };
        let mut tokens = Vec::new();
        if let Some(format) = options.format {
            tokens.push(format.to_string());
        }
        if options.sys_attrs == SysAttrsInclusion::Include {
            tokens.push(SYS_ATTRS_OPTION.to_string());
        }
        if !tokens.is_empty() {
            self.push(ForwardedParameter::Options, tokens.join(","));
        }
    }
}
