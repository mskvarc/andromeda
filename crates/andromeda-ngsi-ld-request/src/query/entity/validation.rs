//! Entity-query semantic validation and option resolution.

use super::parameters::{EntityQueryParameters, EntityQueryParametersOptions};
use crate::{
    attribute_selection::{AttrsAdmission, validate_attrs},
    error::ParameterError,
    linked_retrieval_consistency::{validate_filter_join_depth, validate_projection_join_depth},
};
use andromeda_ngsi_ld_query::type_selection::grammar::TypeExprRaw;

impl EntityQueryParameters {
    /// Parses the `options` and `format` parameters into structured flags.
    #[must_use]
    pub fn parse_options(&self) -> EntityQueryParametersOptions {
        let options = self.options.unwrap_or_default();
        EntityQueryParametersOptions {
            render_mode: self.format.or(options.format).unwrap_or_default(),
            sys_attrs: options.sys_attrs,
            count: self.count.unwrap_or(false),
        }
    }

    /// Whether the query is confined to this broker: `local=true`, or a
    /// `type=*` selector, which Table 5.2.33-1 defines as implying local scope.
    #[must_use]
    pub const fn is_local(&self) -> bool {
        self.local_flag_set() || self.selects_every_type()
    }

    /// Whether `local=true` was given.
    const fn local_flag_set(&self) -> bool {
        matches!(self.local, Some(true))
    }

    /// Whether the type selector is the clause 4.17 wildcard.
    const fn selects_every_type(&self) -> bool {
        matches!(self.entity_type, Some(TypeExprRaw::Wildcard))
    }

    /// Whether any entity filter parameter is present.
    #[must_use]
    pub const fn has_any_filter(&self) -> bool {
        self.entity_type.is_some()
            || self.id.is_some()
            || self.id_pattern.is_some()
            || self.attrs.is_some()
            || self.geo.0.is_some()
            || self.scope_q.is_some()
            || self.q.is_some()
            || !self.entity_selectors.is_empty()
    }

    /// Validate entity-query semantic constraints.
    ///
    /// # Errors
    ///
    /// Returns [`ParameterError::AttrsReservedMember`] when `attrs` names an
    /// Entity member, [`ParameterError::WildcardTypeWithLocalFalse`] when
    /// `type=*` is combined with `local=false`,
    /// [`ParameterError::DropKeepExclusive`] when `drop` and `keep` are both
    /// present, [`ParameterError::OrderingRequiresLocalScope`] when `orderBy`
    /// is sent on a query that is not confined to the local scope,
    /// [`ParameterError::PickOmitExclusive`] and
    /// [`ParameterError::PickOmitWithAttrs`] for the clause 4.21 projection
    /// conflicts, [`ParameterError::LimitZeroWithoutCount`] for `limit=0`
    /// without the `count` option, and the clause 5.7.2.4 depth errors when a
    /// nested projection term or a `q` hop names a Linked Entity the requested
    /// traversal does not reach.
    pub fn validate(&self) -> Result<(), ParameterError> {
        validate_attrs(self.attrs.as_deref(), AttrsAdmission::AttributeNamesOnly)?;
        // Table 5.2.33-1: `type=*` implies local scope, which the request may
        // not then contradict with `local=false`.
        if self.selects_every_type() && matches!(self.local, Some(false)) {
            return Err(ParameterError::WildcardTypeWithLocalFalse);
        }
        if self.drop.is_some() && self.keep.is_some() {
            return Err(ParameterError::DropKeepExclusive);
        }
        // Clause 5.7.2.4: ordering is defined over the local result only.
        if !self.ordering.order_by.is_empty() && !self.is_local() {
            return Err(ParameterError::OrderingRequiresLocalScope);
        }
        // Clause 4.21: pick and omit select opposite ends of the same
        // projection, so a request naming both is contradictory rather than
        // a request the broker can merge.
        if self.pick.is_some() && self.omit.is_some() {
            return Err(ParameterError::PickOmitExclusive);
        }
        if (self.pick.is_some() || self.omit.is_some()) && self.attrs.is_some() {
            return Err(ParameterError::PickOmitWithAttrs);
        }
        // Clause 6.3.10 Table 6.3.10-1: `limit=0` is only meaningful when the
        // request also asks for a count of the matching entities.
        if self.limit == Some(0) && self.count != Some(true) {
            return Err(ParameterError::LimitZeroWithoutCount);
        }
        validate_projection_join_depth(self.pick.as_deref(), self.omit.as_deref(), self.join, self.join_level)?;
        // Clause 5.7.2.4: a `q` hop walks into a Linked Entity, and how far the
        // broker may walk is what the join parameters state.
        validate_filter_join_depth(self.q.as_ref(), self.join, self.join_level)?;
        Ok(())
    }
}
