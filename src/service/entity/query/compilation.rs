//! Compiling request parameters into a [`CompiledEntityQuery`].
//!
//! [`CompiledEntityQuery`]: andromeda_ngsi_ld_query::entity::compiled::CompiledEntityQuery

use crate::{
    core::error::AndromedaError,
    service::entity::{
        compiled_cache::{CompiledQueryKey, LimitCap},
        query::EntityQueryService,
    },
};
use andromeda_jsonld::{context_spec::ContextSpec, term_expansion::ContextualExpander};
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::{
    entity::compiled::CompiledEntityQuery,
    entity_selection::EntitySelection,
    error::QueryError,
    filter::{
        expansion::{apply_expand_values, apply_json_keys, expand_q_expression},
        expression::Expr,
        path::expanded_attribute_name,
    },
    geo::{compile as geo_compile, query::GeoQuery},
    identifier::parse_id_list,
    ordering::{
        collation::IcuCollation,
        compiled::ResultOrdering,
        geometry::OrderingGeometry,
        term::{CompiledOrderTerm, OrderTarget, RawOrderTarget},
    },
    pagination::{CountPolicy, Pagination},
    pattern::Pattern,
    projection::{
        compiled::Projection,
        expansion::{expand_attrs, expand_projection_terms},
    },
    scope::query::ScopeQuery,
    type_selection::{expansion::expand_type_expression, parser as type_parser},
};
use andromeda_ngsi_ld_request::query::{SharedQueryParams, entity::EntityQueryParameters};
use iri_rs::IriBuf;
use std::{collections::HashSet, sync::Arc};

impl EntityQueryService {
    /// Compile an entity query with the default limit cap applied.
    pub async fn compile_query_public(&self, params: &EntityQueryParameters, context: &ContextSpec) -> Result<CompiledEntityQuery, AndromedaError> {
        let compiled = self.compile_query_cached(params, context, LimitCap::Capped).await?;
        Ok(Arc::unwrap_or_clone(compiled))
    }

    /// Compile an entity query without applying a limit cap.
    pub async fn compile_query_uncapped(&self, params: &EntityQueryParameters, context: &ContextSpec) -> Result<CompiledEntityQuery, AndromedaError> {
        let compiled = self.compile_query_cached(params, context, LimitCap::Uncapped).await?;
        Ok(Arc::unwrap_or_clone(compiled))
    }

    /// Cache-aware compilation entry point used by every caller.
    ///
    /// Consults the in-memory cache keyed on the request parameters and the
    /// active JSON-LD context fingerprint; on a miss, compiles and stores.
    pub(super) async fn compile_query_cached(
        &self,
        params: &EntityQueryParameters,
        context: &ContextSpec,
        limit_cap: LimitCap,
    ) -> Result<Arc<CompiledEntityQuery>, AndromedaError> {
        let key = CompiledQueryKey::from_params(params, context, limit_cap);
        self.compiled_cache.get_or_compile(key, || self.compile_query(params, context, limit_cap)).await
    }

    async fn compile_query(&self, params: &EntityQueryParameters, context: &ContextSpec, limit_cap: LimitCap) -> Result<CompiledEntityQuery, AndromedaError> {
        let ids = parse_id_list(&params.parse_ids().unwrap_or_default())?;
        let id_pattern = params
            .id_pattern
            .as_deref()
            .map(Pattern::parse)
            .transpose()
            .map_err(|error| AndromedaError::entity_validation(error.to_string()))?;

        let type_expr = if let Some(raw_types) = params.entity_type.as_deref() {
            let raw = type_parser::parse(raw_types)?;
            Some(expand_type_expression(&raw, &ContextualExpander::new(self.engine(), context)).await?)
        } else {
            None
        };

        let attrs = expand_attrs(&params.parse_attrs(), &ContextualExpander::new(self.engine(), context)).await?;
        let q_expr = self.compile_q_expression(params, context).await?;
        let geo = self.compile_geo_query(params, context).await?;
        let opts = params.parse_options();

        let pick = expand_projection_terms(&params.parse_pick(), &ContextualExpander::new(self.engine(), context)).await?;
        let omit = expand_projection_terms(&params.parse_omit(), &ContextualExpander::new(self.engine(), context)).await?;

        let order_by = self.compile_order_terms(params, context).await?;
        let ordering_geometry = compile_ordering_geometry(params)?;

        // Clause 5.7.2.4: distance ordering has no reference point without a
        // geo-query or an explicit `orderFrom`.
        let has_distance_term = order_by.iter().any(|term| term.direction.is_distance());
        if has_distance_term && geo.is_none() && ordering_geometry.is_none() {
            return Err(AndromedaError::entity_validation(
                "Distance-based ordering requires either a geo-query or orderFrom parameter".to_string(),
            ));
        }

        let pagination = Pagination {
            limit: match limit_cap {
                LimitCap::Capped => params.effective_limit(),
                LimitCap::Uncapped => params.effective_limit_uncapped(),
            },
            offset: params.effective_offset(),
            count: if opts.count { CountPolicy::Emit } else { CountPolicy::Omit },
        };

        let compiled = CompiledEntityQuery::builder()
            .selection(
                EntitySelection::builder()
                    .ids(ids)
                    .id_pattern(id_pattern)
                    .type_expr(type_expr)
                    .attrs(attrs)
                    .q_expr(q_expr)
                    .geo(geo)
                    .scope_q(params.scope_q.as_deref().map(ScopeQuery::parse).transpose()?)
                    .build(),
            )
            .projection(Projection::builder().pick(pick).omit(omit).build())
            .ordering(ResultOrdering::builder().terms(order_by).geometry(ordering_geometry).build())
            .pagination(pagination)
            .build();

        tracing::debug!("Compiled query: {:?}", compiled);

        Ok(compiled)
    }

    /// Expand the `q` filter, then apply the clause 6.4.3.2 `expandValues` and
    /// `jsonKeys` opt-ins to it.
    async fn compile_q_expression(&self, params: &EntityQueryParameters, context: &ContextSpec) -> Result<Option<Expr>, AndromedaError> {
        let Some(q_expr) = expand_q_expression(params.q.as_deref(), &ContextualExpander::new(self.engine(), context)).await? else {
            return Ok(None);
        };

        let q_expr = match params.expand_values {
            Some(ref raw) => {
                let opted_in = self.opted_in_attributes(raw, context).await?;
                apply_expand_values(q_expr, &opted_in, &ContextualExpander::new(self.engine(), context)).await?
            }
            None => q_expr,
        };

        let q_expr = match params.json_keys {
            Some(ref raw) => {
                let opted_in = self.opted_in_attributes(raw, context).await?;
                apply_json_keys(q_expr, &opted_in, &ContextualExpander::new(self.engine(), context)).await?
            }
            None => q_expr,
        };

        Ok(Some(q_expr))
    }

    /// Expand a comma-separated opt-in list into the attribute names a `q`
    /// filter is keyed by (clause 6.4.3.2).
    async fn opted_in_attributes(&self, raw: &str, context: &ContextSpec) -> Result<HashSet<AttributeName>, AndromedaError> {
        let names: Vec<String> = raw.split(',').map(|term| term.trim().to_string()).collect();
        let iris = expand_attrs(&names, &ContextualExpander::new(self.engine(), context)).await?;
        Ok(opted_in_attribute_names(&iris)?)
    }

    /// Compile the `orderBy` terms against the active context (clause 4.23).
    async fn compile_order_terms(&self, params: &EntityQueryParameters, context: &ContextSpec) -> Result<Vec<CompiledOrderTerm>, AndromedaError> {
        let collation = params
            .collation
            .as_deref()
            .map(IcuCollation::parse)
            .transpose()
            .map_err(|error| AndromedaError::entity_validation(error.to_string()))?;

        let mut compiled = Vec::new();
        for term in &params.parse_order_by()? {
            let target = match RawOrderTarget::classify(&term.attr) {
                RawOrderTarget::Id => OrderTarget::Id,
                RawOrderTarget::Type => OrderTarget::Type,
                RawOrderTarget::Attribute(name) => OrderTarget::Attribute(self.expanded_key(name, context).await?),
            };
            compiled.push(CompiledOrderTerm {
                target,
                direction: term.direction,
                collation: collation.clone(), // clone: every term carries its own copy of the query's collation
            });
        }
        Ok(compiled)
    }

    /// Compile the `georel` / `geometry` / `coordinates` triple (clause 4.10).
    async fn compile_geo_query(&self, params: &EntityQueryParameters, context: &ContextSpec) -> Result<Option<GeoQuery>, AndromedaError> {
        geo_compile::compile_geo_query(
            params.georel.as_deref(),
            params.geometry.as_deref(),
            params.coordinates.as_deref(),
            params.geoproperty.as_deref(),
            &ContextualExpander::new(self.engine(), context),
        )
        .await
        .map_err(AndromedaError::from)
    }
}

/// Compile `orderFrom` and `orderGeometry` into the reference geometry a
/// distance ordering measures from (clause 5.2.43).
fn compile_ordering_geometry(params: &EntityQueryParameters) -> Result<Option<OrderingGeometry>, AndromedaError> {
    let Some(ref order_from) = params.order_from else {
        return Ok(None);
    };
    let coordinates: serde_json::Value = serde_json::from_str(order_from)
        .map_err(|_| AndromedaError::entity_validation(format!("Invalid orderFrom value: '{order_from}' is not valid JSON")))?;
    let geometry_type = params.order_geometry.as_deref().unwrap_or("Point");
    let geojson = serde_json::json!({ "type": geometry_type, "coordinates": coordinates });
    let geometry = Geometry::from_geojson_value(&geojson)
        .map_err(|error| AndromedaError::entity_validation(format!("Invalid orderFrom/orderGeometry combination: {error}")))?;
    Ok(Some(OrderingGeometry::new(geometry)))
}

/// Read an expanded `expandValues` / `jsonKeys` selection as the attribute
/// names a `q` filter is keyed by (clause 6.4.3.2).
fn opted_in_attribute_names(iris: &[IriBuf]) -> Result<HashSet<AttributeName>, QueryError> {
    iris.iter().map(expanded_attribute_name).collect()
}
