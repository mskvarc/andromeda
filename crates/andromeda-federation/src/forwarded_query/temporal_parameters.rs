//! The temporal parameters a forwarded temporal query carries (clauses 5.7.3.4,
//! 5.7.4.4, 6.3.12).

use crate::{
    forwarded_parameter::ForwardedParameter,
    forwarded_query::{ForwardedQuery, SYS_ATTRS_OPTION},
    request::TemporalAnswer,
};
use andromeda_ngsi_ld_lexicon::{temporal_format::TemporalFormat, time::rendering::instant_text, time_prop::TimeProp};
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
        self.push_optional(ForwardedParameter::TimeAt, params.time_at.map(instant_text));
        self.push_optional(ForwardedParameter::EndTimeAt, params.end_time_at.map(instant_text));
        self.push_optional(ForwardedParameter::Timeproperty, params.timeproperty.map(|property| property.to_string()));
        match answer {
            TemporalAnswer::AsRequested => self.push_requested_representation(params),
            TemporalAnswer::Instances => self.push_instance_representation(params),
        }
        self.push_traversal(params.join.map(|join| join.to_string()), params.join_level, params.contained_by.as_deref());
    }

    /// The representation and aggregation the client asked for.
    ///
    /// An aggregated representation (clause 4.5.19) is asked of the source as
    /// it was asked of this broker: an aggregate of a part is merged as the
    /// aggregate it is. Every other representation is asked as the normalized
    /// instances of the window (clause 4.5.7), whatever `format` or `options`
    /// token the client used: the answers of every holder are gathered as
    /// instances and written once, in the representation the client asked for,
    /// so `temporalValues` pairs are never merged as arrays.
    fn push_requested_representation(&mut self, params: &TemporalQueryParameters) {
        let format = params.selected_format();
        self.push_optional(ForwardedParameter::LastN, params.last_n.map(|last_n| last_n.to_string()));
        match format {
            // The grouped extension format is never forwarded: grouping is applied by this broker after the merge, so a source is asked for the plain aggregated representation and `aggrGroupBy` stays local.
            TemporalFormat::AggregatedValues | TemporalFormat::GroupedValues => {
                self.push(ForwardedParameter::Format, TemporalFormat::AggregatedValues.to_string());
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
            TemporalFormat::Normalized | TemporalFormat::TemporalValues => {}
        }
        self.push_instance_representation(params);
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

    /// A `temporalValues` read is asked of a source as the normalized instances
    /// of the window, so the answers of every holder merge as instances.
    #[test]
    fn a_temporal_values_read_is_asked_as_instances() {
        let requested = forwarded(
            "timerel=after&timeAt=2020-01-01T00:00:00Z&format=temporalValues&lastN=3",
            TemporalAnswer::AsRequested,
        );
        pretty_assertions::assert_eq!(requested, "timerel=after&timeAt=2020-01-01T00%3A00%3A00Z&lastN=3");
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
