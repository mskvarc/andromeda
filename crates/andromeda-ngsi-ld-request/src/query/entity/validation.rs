//! Entity-query semantic validation and option resolution.

use super::parameters::{EntityQueryParameters, EntityQueryParametersOptions};
use crate::{attribute_selection::validate_attrs, error::ParameterError};

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
        Ok(())
    }
}
