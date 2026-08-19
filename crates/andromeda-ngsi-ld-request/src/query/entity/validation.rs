//! Entity-query semantic validation and option resolution.

use super::parameters::{EntityQueryParameters, EntityQueryParametersOptions};
use crate::{
    attribute_selection::validate_attrs,
    error::ParameterError,
    linked_retrieval_consistency::{validate_filter_join_depth, validate_projection_join_depth},
};

impl EntityQueryParameters {
    /// Parses the `options` and `format` parameters into structured flags.
    pub fn parse_options(&self) -> EntityQueryParametersOptions {
        let options = self.options.unwrap_or_default();
        EntityQueryParametersOptions {
            render_mode: self.format.or(options.format).unwrap_or_default(),
            sys_attrs: options.sys_attrs,
            count: self.count.unwrap_or(false),
        }
    }

    /// Whether any entity filter parameter is present.
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
    pub fn validate(&self) -> Result<(), ParameterError> {
        validate_attrs(self.attrs.as_deref())?;
        if self.drop.is_some() && self.keep.is_some() {
            return Err(ParameterError::DropKeepExclusive);
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
