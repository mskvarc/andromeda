//! Entity-query semantic validation and option resolution.

use super::parameters::{EntityQueryParameters, EntityQueryParametersOptions};
use crate::{
    error::ParameterError,
    representation_params::{resolve_response_format, validate_representation_params},
};
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;

impl EntityQueryParameters {
    /// Parses the `options` and `format` parameters into structured flags.
    pub fn parse_options(&self) -> EntityQueryParametersOptions {
        let opts = self.options.as_deref().unwrap_or("");
        EntityQueryParametersOptions {
            render_mode: resolve_response_format(self.format, self.options.as_deref()),
            sys_attrs: if opts.split(',').map(str::trim).any(|token| token == "sysAttrs") {
                SysAttrsInclusion::Include
            } else {
                SysAttrsInclusion::Strip
            },
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
    }

    /// Validate entity-query semantic constraints.
    pub fn validate(&self) -> Result<(), ParameterError> {
        validate_representation_params(self.format, self.options.as_deref())?;
        if let Some(ref order_geometry) = self.order_geometry {
            match order_geometry.as_str() {
                "Point" | "MultiPoint" | "LineString" | "MultiLineString" | "Polygon" | "MultiPolygon" => {}
                "GeometryCollection" => return Err(ParameterError::OrderGeometryCollection),
                other => return Err(ParameterError::OrderGeometryInvalid(other.to_string())),
            }
        }
        if let Some(ref order_from) = self.order_from {
            serde_json::from_str::<serde_json::Value>(order_from).map_err(|source| ParameterError::OrderFromInvalid {
                value: order_from.clone(),
                source,
            })?;
        }
        if self.drop.is_some() && self.keep.is_some() {
            return Err(ParameterError::DropKeepExclusive);
        }
        Ok(())
    }
}
