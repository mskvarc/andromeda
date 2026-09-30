//! The name-bearing filters a forwarded query carries -- the clause 4.9 query
//! with its `jsonKeys`, the context source filter and the clause 4.10 geo-query
//! -- each spelled in the registration's forwarding context (clause 4.3.6.6).

use crate::{
    error::FederationError,
    federation_invariant::FederationInvariant,
    forwarded_names::ForwardedNames,
    forwarded_parameter::ForwardedParameter,
    forwarded_query::ForwardedQuery,
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::{filter::expression::Expr, geo::input::GeoQueryInput};
use serde_json::Value;

impl ForwardedQuery {
    /// Write the clause 4.9 query, spelled in the forwarding context, and the
    /// `jsonKeys` that tell the source which of its members are literal JSON
    /// keys (clause 4.9 EXAMPLE 11).
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::Query`] when a name in the query does not
    /// expand against the request context, and [`FederationError::JsonLd`] or
    /// [`FederationError::Internal`] when a `jsonKeys` name does not spell in
    /// the forwarding context.
    pub(super) async fn push_q(&mut self, q: Option<&Expr>, json_keys: Option<&[AttributeName]>, names: &ForwardedNames<'_>) -> Result<(), FederationError> {
        let Some(q) = q else {
            return Ok(());
        };
        self.push(ForwardedParameter::Q, names.q(q, json_keys).await?.to_string());
        if let Some(json_keys) = json_keys.filter(|keys| !keys.is_empty()) {
            let mut spelled = Vec::with_capacity(json_keys.len());
            for name in json_keys {
                spelled.push(names.attr(name).await?.to_string());
            }
            self.push(ForwardedParameter::JsonKeys, spelled.join(","));
        }
        Ok(())
    }

    /// Write the context source filter, whose names are Context Source Property
    /// names expanded like a clause 4.9 query's, spelled in the forwarding
    /// context.
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::Query`] when a name in the filter does not
    /// expand against the request context.
    pub(super) async fn push_csf(&mut self, csf: Option<&Expr>, names: &ForwardedNames<'_>) -> Result<(), FederationError> {
        if let Some(csf) = csf {
            self.push(ForwardedParameter::Csf, names.q(csf, None).await?.to_string());
        }
        Ok(())
    }

    /// Write the four clause 4.10 geo-query members, the `GeoProperty` spelled
    /// in the forwarding context.
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::JsonLd`] or [`FederationError::Internal`]
    /// when the `GeoProperty` does not spell in the forwarding context, and
    /// [`FederationError::Internal`] when the admitted geometry cannot be
    /// written back to its wire form.
    pub(super) async fn push_geo(&mut self, geo: Option<&GeoQueryInput>, names: &ForwardedNames<'_>) -> Result<(), FederationError> {
        let Some(geo) = geo else {
            return Ok(());
        };
        // A geometry that cannot travel is a broken invariant, never a reason to
        // forward the query without its geo filter: the source would answer
        // entities the client's filter excludes.
        let geometry = serde_json::to_value(&geo.geometry).map_err(FederationInvariant::ForwardedGeometry)?;
        let (Some(geometry_type), Some(coordinates)) = (geometry.get("type").and_then(Value::as_str), geometry.get("coordinates")) else {
            return Err(FederationInvariant::ForwardedGeometryShape.into());
        };
        self.push(ForwardedParameter::Georel, geo.georel.to_string());
        self.push(ForwardedParameter::Geometry, geometry_type.to_string());
        self.push(ForwardedParameter::Coordinates, coordinates.to_string());
        if let Some(geoproperty) = &geo.geoproperty {
            self.push(ForwardedParameter::Geoproperty, names.geoproperty(geoproperty).await?.to_string());
        }
        Ok(())
    }
}
