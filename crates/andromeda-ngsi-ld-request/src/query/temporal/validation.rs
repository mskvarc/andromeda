//! What a temporal query has to satisfy before it runs (clauses 5.7.3.4, 5.7.4.4).
//!
//! The parameter set beside this one is what a request wrote; this is what the
//! specification makes of it -- the representation it selects, the execution
//! scope it asked for, the narrowing inputs clause 5.7.4.4 counts, and the
//! constraints a retrieval and a query each have to meet.

use crate::{
    attribute_selection::{AttrsAdmission, validate_attrs},
    error::ParameterError,
    linked_retrieval_consistency::{validate_filter_join_depth, validate_projection_join_depth},
    query::temporal::TemporalQueryParameters,
    query_narrowing::{AdmittedNarrowings, ExecutionScope, NarrowingInputs, require_narrowing},
    representation_parameter::{WrittenValue, resolve_representation, selected_representation},
};
use andromeda_ngsi_ld_lexicon::{temporal_format::TemporalFormat, time_prop::TimeProp};
use andromeda_ngsi_ld_query::{
    projection::exclusivity::{Presence, ProjectionSelectors, check_projection_exclusivity},
    temporal::{
        grouping::{AggregationGrouping, group_key::GroupKey},
        ordering::TemporalEntityOrdering,
        window::{BoundPresence, TimeInterval},
    },
    type_selection::grammar::TypeExprRaw,
};
use andromeda_ngsi_ld_representation::{join_mode::JoinMode, sys_attrs::SysAttrsInclusion};

/// Which Entity members `attrs` admits at each of the two temporal endpoints.
///
/// Both tables restrict `attrs` to "an Attribute (Property or Relationship)
/// name", and clause 4.18 makes the Scope "a special scope Property that is
/// non-reified in the normalized Entity representation and reified in the
/// temporal representation of an Entity" — so on a temporal answer `scope` is
/// an Attribute name and Table 6.18.3.2-1 excludes nothing further. Table
/// 6.19.3.1-1 excludes it explicitly on the retrieval, "except that id, type,
/// scope are not allowed", and an explicit prohibition beats an inference from
/// the representation, so the same spelling is a bad request there.
const fn attrs_admission(require_temporal: bool) -> AttrsAdmission {
    if require_temporal {
        AttrsAdmission::ReifiedScopeAdmitted
    } else {
        AttrsAdmission::AttributeNamesOnly
    }
}

impl TemporalQueryParameters {
    /// Returns whether the query should be local-only (no distributed forwarding).
    ///
    /// Per NGSI-LD Table 6.18.3.2-1, `type=*` implies `local=true` since wildcard
    /// type queries are undefined across federated sources.
    #[must_use]
    pub fn is_local(&self) -> bool {
        if self.local == Some(true) {
            return true;
        }
        // type=* implies local=true per spec Table 6.18.3.2-1
        matches!(self.entity_type, Some(TypeExprRaw::Wildcard))
    }

    /// The temporal property the window applies to, defaulting to `observedAt`
    /// (clause 4.8) when the request named none. The value was validated as a
    /// [`TimeProp`] at the query boundary.
    #[must_use]
    pub fn timeproperty_or_default(&self) -> TimeProp {
        self.timeproperty.unwrap_or_default()
    }

    /// Parses the temporal response format from `format` and `options` parameters.
    ///
    /// The `format` parameter takes precedence over `options`. Recognized values
    /// are `"temporalValues"` and `"aggregatedValues"`. Defaults to
    /// `TemporalFormat::Normalized`.
    ///
    /// # Errors
    ///
    /// Returns [`ParameterError::FormatInvalid`] or
    /// [`ParameterError::OptionsInvalid`] when either parameter carried a value
    /// Table 6.3.12-1 does not define (clause 6.3.20).
    pub fn parse_format(&self) -> Result<TemporalFormat, ParameterError> {
        resolve_representation(self.format.as_ref(), self.options.as_ref())
    }

    /// Whether the response carries the system-generated temporal attributes
    /// (clause 6.3.11 `options=sysAttrs`).
    ///
    /// Reads a supported `options` only, which `validate` has already required.
    #[must_use]
    pub fn sys_attrs(&self) -> SysAttrsInclusion {
        self.options
            .as_ref()
            .and_then(WrittenValue::if_supported)
            .map_or(SysAttrsInclusion::Strip, |options| options.sys_attrs)
    }

    /// The execution scope the request asked for (clause 5.5.13).
    #[must_use]
    pub fn execution_scope(&self) -> ExecutionScope {
        if self.is_local() {
            ExecutionScope::Local
        } else {
            ExecutionScope::Distributed
        }
    }

    /// The clause 5.7.4.4 narrowing inputs this query carries.
    #[must_use]
    pub fn narrowing_inputs(&self) -> NarrowingInputs<'_> {
        NarrowingInputs::builder()
            .admitted(AdmittedNarrowings::ENTITIES)
            .entity_type(self.entity_type.as_ref())
            .entity_selectors(&self.entity_selectors)
            .attrs(self.attrs.as_deref())
            .pick(self.pick.as_deref())
            .q(self.q.as_ref())
            .geo(self.geo.0.as_ref())
            .local_scope(self.execution_scope())
            .build()
    }

    /// The temporal representation a validated request selected.
    ///
    /// The infallible counterpart of [`Self::parse_format`], read once the
    /// request has been validated and there is no rejection left to make.
    #[must_use]
    pub fn selected_format(&self) -> TemporalFormat {
        selected_representation(self.format.as_ref(), self.options.as_ref())
    }

    /// Returns whether the query requests simplified (key-values) output.
    ///
    /// Checks both `keyValues` and `simplified` option values, which are
    /// equivalent per NGSI-LD spec.
    #[must_use]
    pub const fn is_simplified(&self) -> bool {
        false
    }

    /// Validate parameters.
    /// - For query endpoint (`require_temporal=true)`: timerel and timeAt are REQUIRED.
    /// - For single-entity GET (`require_temporal=false)`: timerel and timeAt are optional (both or neither).
    ///
    /// # Errors
    ///
    /// Returns [`ParameterError::AttrsReservedMember`] when `attrs` names an
    /// Entity member, [`ParameterError::Projection`] for the clause 4.21
    /// selector conflicts, the clause 5.7.2.4 depth errors when a nested projection
    /// term or a `q` hop names a Linked Entity the requested traversal does
    /// not reach, [`ParameterError::InlineJoinWithoutInstances`] when
    /// `join=inline` meets a representation with no instance to nest under,
    /// [`ParameterError::Query`] carrying a temporal-ordering failure for an
    /// ordering clause 5.7.4.3 does not admit,
    /// [`ParameterError::TimerelTimeatRequired`] and
    /// [`ParameterError::SelectorRequired`] when a query endpoint omits the
    /// temporal window or every entity selector,
    /// [`ParameterError::Query`] when the `timerel`, `timeAt` and `endTimeAt`
    /// group is incomplete, and [`ParameterError::AggrMethodsRequired`] when
    /// `format=aggregatedValues` carries no aggregation methods.
    pub fn validate(&self, require_temporal: bool) -> Result<(), ParameterError> {
        // Clause 6.3.20: a `format` or `options` value Table 6.3.12-1 does not
        // define is refused here rather than during deserialisation, where the
        // error type would have been lost (see `crate::representation_parameter`).
        self.parse_format()?;
        validate_attrs(self.attrs.as_deref(), attrs_admission(require_temporal))?;
        // Clause 4.21: the temporal API carries the same three selectors and
        // inherits the rule about combining them unchanged.
        check_projection_exclusivity(ProjectionSelectors {
            pick: Presence::of(self.pick.as_deref()),
            omit: Presence::of(self.omit.as_deref()),
            attrs: Presence::of(self.attrs.as_deref()),
        })?;
        // Clauses 5.7.3.4 and 5.7.4.4 admit no Linked Entity retrieval on the
        // temporal operations at all, raising an error where projection
        // attributes or filter conditions indicate one. Andromeda's join
        // extension accepts them instead, and holds them to the bound clause
        // 5.7.2.4 sets on the entity bindings: a nested projection term and a
        // `q` hop each reach only as far as the requested join walks.
        validate_projection_join_depth(self.pick.as_deref(), self.omit.as_deref(), self.join, self.join_level)?;
        validate_filter_join_depth(self.q.as_ref(), self.join, self.join_level)?;
        // Clause 4.5.23 nests a linked entity under the attribute instance that
        // names it. The simplified representation of clause 4.5.5 and the
        // aggregated one of clause 4.5.19 both collapse instances into value
        // arrays, leaving nothing to nest under, so `join=inline` has no
        // meaning there and is refused rather than silently ignored. A flat
        // join stays available: it puts the linked entities beside the roots
        // and needs no nesting site.
        if self.join == Some(JoinMode::Inline) {
            let format = self.parse_format()?;
            match format {
                // The grouped extension format collapses instances the same way
                // the aggregated one does, so it has no nesting site either.
                TemporalFormat::TemporalValues | TemporalFormat::AggregatedValues | TemporalFormat::GroupedValues => {
                    return Err(ParameterError::InlineJoinWithoutInstances(format));
                }
                TemporalFormat::Normalized => {}
            }
        }
        // Which orderings clause 5.7.4.3 admits is a property of the ordering
        // itself; the request seam asks the compiled form rather than
        // restating the rule, and compilation later reads the same answer.
        TemporalEntityOrdering::of_parameters(&self.ordering).map_err(ParameterError::Query)?;
        if require_temporal {
            if self.timerel.is_none() || self.time_at.is_none() {
                return Err(ParameterError::TimerelTimeatRequired);
            }
            // Clause 5.7.4.4 states the clause 5.7.2.4 rule word for word,
            // local-scope exemption included.
            require_narrowing(&self.narrowing_inputs())?;
        }
        // The `timerel`/`timeAt`/`endTimeAt` combination rule is owned by the
        // query crate, so the request seam asks rather than re-deriving it.
        TimeInterval::check_time_bounds(
            self.timerel,
            BoundPresence::of(self.time_at.as_ref()),
            BoundPresence::of(self.end_time_at.as_ref()),
        )
        .map_err(ParameterError::Query)?;
        // Both the standard aggregated format and the grouped extension format
        // apply the clause 4.5.19 methods, so both require them.
        if self.parse_format()?.is_aggregated() && self.aggr_methods.is_none() {
            return Err(ParameterError::AggrMethodsRequired);
        }
        self.validate_grouping()?;
        Ok(())
    }

    /// Validate the `aggrGroupBy` grouping extension parameter (study §4.9).
    ///
    /// The parameter is inert unless present, so a broker with the extension
    /// disabled — where the HTTP gate has already refused a non-empty value —
    /// reaches here only with `aggr_group_by` absent and passes unchanged.
    ///
    /// # Errors
    ///
    /// Returns [`ParameterError::AggrGroupByWithoutGroupedValues`] when grouping
    /// is asked for outside the grouped format,
    /// [`ParameterError::AggrGroupByInvalid`] when the value is not a well-formed
    /// grouping, [`ParameterError::LastNWithGrouping`] when it is combined with
    /// `lastN`, and [`ParameterError::AggrGroupByOnAggregatedAttribute`] when a
    /// key names an attribute the request also aggregates.
    fn validate_grouping(&self) -> Result<(), ParameterError> {
        let Some(raw) = self.aggr_group_by.as_deref() else {
            return Ok(());
        };
        if !self.parse_format()?.is_grouped() {
            return Err(ParameterError::AggrGroupByWithoutGroupedValues);
        }
        let grouping = AggregationGrouping::parse(raw).map_err(ParameterError::AggrGroupByInvalid)?;
        if self.last_n.is_some() {
            return Err(ParameterError::LastNWithGrouping);
        }
        // Self-reference: grouping by an attribute the request also aggregates
        // is ambiguous, so it must be spelled out rather than guessed. Compared
        // on the compact names both parameters carry at this stage.
        if let Some(attrs) = self.attrs.as_deref() {
            for term in grouping.terms() {
                if let GroupKey::Path(path) = &term.key
                    && attrs.iter().any(|attr| attr.as_str() == path.attr.as_str())
                {
                    // clone: the error owns the offending name past the borrow of the parsed grouping.
                    return Err(ParameterError::AggrGroupByOnAggregatedAttribute(path.attr.clone()));
                }
            }
        }
        Ok(())
    }
}
