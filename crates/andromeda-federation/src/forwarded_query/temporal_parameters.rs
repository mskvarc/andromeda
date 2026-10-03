//! The temporal parameters a forwarded temporal query carries (clauses 5.7.3.4,
//! 5.7.4.4, 6.3.12).

use crate::{
    forwarded_parameter::ForwardedParameter,
    forwarded_query::{ForwardedQuery, SYS_ATTRS_OPTION},
    request::TemporalAnswer,
};
use andromeda_ngsi_ld_lexicon::{rfc3339::sub_second, temporal_format::TemporalFormat, time_prop::TimeProp};
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use andromeda_ngsi_ld_request::{query::temporal::TemporalQueryParameters, representation_parameter::WrittenValue};

impl ForwardedQuery {
    /// The temporal window, representation and aggregation parameters a temporal
    /// query forwards (clauses 5.7.3.4, 5.7.4.4, 6.3.12), and its clause 4.5.23
    /// traversal continuation.
    ///
    /// A source asked for [`TemporalAnswer::Instances`] is sent the window
    /// alone: no `format`, no aggregation and no `lastN`, so it answers with
    /// every normalized instance of the window, and `sysAttrs` whenever the
    /// window is positioned on a system time, so every instance carries the
    /// instant it is aggregated on.
    pub(super) fn push_temporal_parameters(&mut self, params: &TemporalQueryParameters, answer: TemporalAnswer) {
        self.push_optional(ForwardedParameter::Timerel, params.timerel.map(|timerel| timerel.to_string()));
        self.push_optional(ForwardedParameter::TimeAt, params.time_at.map(sub_second));
        self.push_optional(ForwardedParameter::EndTimeAt, params.end_time_at.map(sub_second));
        self.push_optional(ForwardedParameter::Timeproperty, params.timeproperty.map(|property| property.to_string()));
        match answer {
            TemporalAnswer::AsRequested => self.push_requested_representation(params),
            TemporalAnswer::Instances => self.push_instance_representation(params),
        }
        self.push_traversal(params.join.map(|join| join.to_string()), params.join_level, params.contained_by.as_deref());
    }

    /// The representation and aggregation the client asked for.
    fn push_requested_representation(&mut self, params: &TemporalQueryParameters) {
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
    }

    /// The normalized instance representation, with the system times when the
    /// window is positioned on one (clause 4.8).
    fn push_instance_representation(&mut self, params: &TemporalQueryParameters) {
        let requested = params
            .options
            .as_ref()
            .and_then(WrittenValue::if_supported)
            .is_some_and(|options| options.sys_attrs == SysAttrsInclusion::Include);
        let positioned_on_a_system_time = match params.timeproperty_or_default() {
            TimeProp::Created | TimeProp::Modified => true,
            TimeProp::Observed | TimeProp::Deleted => false,
        };
        if requested || positioned_on_a_system_time {
            self.push(ForwardedParameter::Options, SYS_ATTRS_OPTION.to_string());
        }
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

#[cfg(test)]
mod tests {
    use crate::{forwarded_query::ForwardedQuery, request::TemporalAnswer};
    use andromeda_ngsi_ld_request::query::temporal::TemporalQueryParameters;

    fn forwarded(query: &str, answer: TemporalAnswer) -> String {
        let params: TemporalQueryParameters = serde_urlencoded::from_str(query).expect("a valid temporal query");
        let mut forwarded = ForwardedQuery::default();
        forwarded.push_temporal_parameters(&params, answer);
        forwarded.to_query_string()
    }

    /// Clause 5.7.4.4: an aggregate of an entity held in several parts is one
    /// of its merged instances, so a source is asked for the instances of the
    /// window rather than for an aggregate, a format or a `lastN` of its own.
    #[test]
    fn the_instance_representation_drops_every_aggregation_parameter() {
        let query = "timerel=after&timeAt=2020-01-01T00:00:00Z&format=aggregatedValues&aggrMethods=avg,max&aggrPeriodDuration=PT1H&lastN=5";
        pretty_assertions::assert_eq!(forwarded(query, TemporalAnswer::Instances), "timerel=after&timeAt=2020-01-01T00%3A00%3A00Z");
        let requested = forwarded(query, TemporalAnswer::AsRequested);
        assert!(
            requested.contains("aggrMethods=avg%2Cmax") && requested.contains("format=aggregatedValues"),
            "{requested}"
        );
    }

    /// Clause 4.8: an instance carries its system times only when asked, so a
    /// window positioned on one asks for them.
    #[test]
    fn a_non_observed_timeproperty_asks_for_system_attributes() {
        pretty_assertions::assert_eq!(
            forwarded("timerel=after&timeAt=2020-01-01T00:00:00Z&timeproperty=modifiedAt", TemporalAnswer::Instances),
            "timerel=after&timeAt=2020-01-01T00%3A00%3A00Z&timeproperty=modifiedAt&options=sysAttrs"
        );
        pretty_assertions::assert_eq!(
            forwarded("timerel=after&timeAt=2020-01-01T00:00:00Z", TemporalAnswer::Instances),
            "timerel=after&timeAt=2020-01-01T00%3A00%3A00Z"
        );
    }
}
