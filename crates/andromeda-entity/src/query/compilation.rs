//! Compiling request parameters into a [`CompiledEntityQuery`].
//!
//! [`CompiledEntityQuery`]: andromeda_ngsi_ld_query::entity::compiled::CompiledEntityQuery

use crate::{
    compiled_cache::{CompiledQueryKey, LimitCap},
    entity_validation_reason::EntityValidationReason,
    error::EntityError,
    query::{EntityQueryService, selector_compilation::compile_entity_selectors},
};
use andromeda_jsonld::{context_spec::ContextSpec, engine::term_lookup::PreparedTermContext, term_expansion::ContextualExpander};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::{
    entity::compiled::CompiledEntityQuery,
    entity_selection::{ConsideredEntities, EntitySelection},
    filter::{
        expansion::{apply_expand_values, expand_attr_path, expand_attribute_names, expand_q_expr},
        expression::Expr,
        json_key_attributes::JsonKeyAttributes,
    },
    geo::{compile as geo_compile, query::GeoQuery},
    ordering::{
        compiled::ResultOrdering,
        geometry::OrderingGeometry,
        term::{CompiledOrderTerm, OrderTarget, RawOrderTarget},
    },
    pagination::{CountPolicy, Pagination},
    projection::{
        compiled::Projection,
        expansion::{expand_attrs, expand_projection_terms},
    },
};
use andromeda_ngsi_ld_request::query::{SharedQueryParams, entity::parameters::EntityQueryParameters};
use std::{collections::HashSet, sync::Arc};

impl EntityQueryService {
    /// Compile an entity query with the default limit cap applied.
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::JsonLd`] when a term of the request cannot be
    /// expanded against `context`, [`EntityError::Query`] when the `q`, `geoQ`
    /// or `orderBy` expression is not well formed, and
    /// [`EntityError::Request`] when a query parameter cannot be interpreted.
    pub async fn compile_query_public(&self, params: &EntityQueryParameters, context: &ContextSpec) -> Result<CompiledEntityQuery, EntityError> {
        let compiled = self.compile_query_cached(params, context, LimitCap::Capped).await?;
        Ok(Arc::unwrap_or_clone(compiled))
    }

    /// Compile an entity query without applying a limit cap.
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::JsonLd`] when a term of the request cannot be
    /// expanded against `context`, [`EntityError::Query`] when the `q`, `geoQ`
    /// or `orderBy` expression is not well formed, and
    /// [`EntityError::Request`] when a query parameter cannot be interpreted.
    pub async fn compile_query_uncapped(&self, params: &EntityQueryParameters, context: &ContextSpec) -> Result<CompiledEntityQuery, EntityError> {
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
    ) -> Result<Arc<CompiledEntityQuery>, EntityError> {
        let key = CompiledQueryKey::from_params(params, context, limit_cap);
        self.compiled_cache.get_or_compile(key, || self.compile_query(params, context, limit_cap)).await
    }

    async fn compile_query(&self, params: &EntityQueryParameters, context: &ContextSpec, limit_cap: LimitCap) -> Result<CompiledEntityQuery, EntityError> {
        let prepared_context = self.engine().prepare_term_context(context);
        let selectors = compile_entity_selectors(
            self.engine(),
            &prepared_context,
            &params.entity_selectors,
            params.id.clone().unwrap_or_default(),
            params.id_pattern.as_ref(),
            params.entity_type.as_ref(),
        )
        .await?;

        let expander = ContextualExpander::new(self.engine(), &prepared_context);
        let attrs = expand_attrs(params.attrs.as_deref().unwrap_or_default(), &expander).await?;
        let q_expr = self.compile_q_expression(params, &prepared_context).await?;
        let geo = self.compile_geo_query(params, &prepared_context).await?;
        let opts = params.parse_options()?;

        let pick = expand_projection_terms(params.pick.as_deref().unwrap_or_default(), &expander).await?;
        let omit = expand_projection_terms(params.omit.as_deref().unwrap_or_default(), &expander).await?;

        let order_by = self.compile_order_terms(params, &prepared_context).await?;
        let ordering_geometry = compile_ordering_geometry(params);

        // Clause 5.7.2.4: distance ordering has no reference point without a
        // geo-query or an explicit `orderFrom`.
        let has_distance_term = order_by.iter().any(|term| term.direction.is_distance());
        if has_distance_term && geo.is_none() && ordering_geometry.is_none() {
            return Err(EntityError::Validation(EntityValidationReason::DistanceOrderingWithoutReference));
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
                    .selectors(selectors)
                    .attrs(attrs)
                    .q_expr(q_expr)
                    .geo(geo)
                    .scope_q(params.scope_q.clone())
                    // Clause 5.5.14: a request narrowed to an empty identifier
                    // set considers no entity, which the selectors' identifier
                    // lists cannot say on their own.
                    .considered(ConsideredEntities::of_request(params.id.as_deref()))
                    .build(),
            )
            .projection(Projection::builder().pick(pick).omit(omit).build())
            .ordering(ResultOrdering::builder().terms(order_by).geometry(ordering_geometry).build())
            .pagination(pagination)
            .build();

        tracing::debug!("Compiled query: {:?}", compiled);

        Ok(compiled)
    }

    /// Expand the `q` filter under the clause 4.9 `jsonKeys` declaration, then
    /// apply the `expandValues` coercion to the result.
    async fn compile_q_expression(&self, params: &EntityQueryParameters, prepared_context: &PreparedTermContext) -> Result<Option<Expr>, EntityError> {
        let expander = ContextualExpander::new(self.engine(), prepared_context);
        // Expansion consumes its AST; the cache-owned request remains immutable across lookups.
        let Some(q_expr) = params.q.clone() else {
            return Ok(None);
        };
        // `jsonKeys` decides how the filter's own terms expand, so it is
        // resolved before the expansion rather than applied to its result.
        let json_keys: JsonKeyAttributes = match params.json_keys {
            Some(ref raw) => expand_attribute_names(raw, &expander).await?.into_iter().collect(),
            None => JsonKeyAttributes::default(),
        };
        let q_expr = expand_q_expr(q_expr, &json_keys, &expander).await?;

        let q_expr = match params.expand_values {
            Some(ref raw) => {
                let opted_in: HashSet<AttributeName> = expand_attribute_names(raw, &expander).await?.into_iter().collect();
                apply_expand_values(q_expr, &opted_in, &expander).await?
            }
            None => q_expr,
        };

        Ok(Some(q_expr))
    }

    /// Compile the `orderBy` terms against the active context (clause 4.23).
    ///
    /// Every name of an ordering path is expanded, not just its head. Clause
    /// 4.9 requires it of the attribute path clause 4.23.3 borrows — "Every name
    /// in the list shall be expanded to a URI (Fully Qualified Name) as mandated
    /// by clause 5.5.7" — and the members of a compound value expand for the
    /// same reason, because expansion turns them into IRI-keyed nodes of the
    /// graph. Expanding the head alone addressed `city` in a payload holding
    /// the expansion of `city`, and ordered by nothing.
    ///
    /// The path is expanded under no `jsonKeys` declaration: an attribute whose
    /// value is opaque JSON keeps literal member keys, and the ordering reads
    /// that spelling from the path's own pre-expansion copy rather than from a
    /// declaration `orderBy` has no parameter for.
    async fn compile_order_terms(&self, params: &EntityQueryParameters, context: &PreparedTermContext) -> Result<Vec<CompiledOrderTerm>, EntityError> {
        // Parsed and validated at the query boundary (clause 4.23).
        let collation = params.ordering.collation.clone();
        let expander = ContextualExpander::new(self.engine(), context);
        let json_keys = JsonKeyAttributes::default();

        let mut compiled = Vec::new();
        let order_terms = &params.ordering.order_by;
        for term in order_terms {
            let target = match RawOrderTarget::classify(&term.attr) {
                RawOrderTarget::Id => OrderTarget::Id,
                RawOrderTarget::Type => OrderTarget::Type,
                RawOrderTarget::Scope => OrderTarget::Scope,
                // clone: expansion rewrites the path it consumes, and the cached
                // request parameters stay immutable across compilations.
                RawOrderTarget::Attribute(path) => OrderTarget::Attribute(expand_attr_path(path.clone(), &json_keys, &expander).await?),
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
    async fn compile_geo_query(&self, params: &EntityQueryParameters, context: &PreparedTermContext) -> Result<Option<GeoQuery>, EntityError> {
        geo_compile::compile_geo_query(params.geo.0.as_ref(), &ContextualExpander::new(self.engine(), context))
            .await
            .map_err(EntityError::from)
    }
}

/// Compile `orderFrom` and `orderGeometry` into the reference geometry a
/// distance ordering measures from (clause 5.2.43).
fn compile_ordering_geometry(params: &EntityQueryParameters) -> Option<OrderingGeometry> {
    // The compiled query owns the geometry independently of the reusable request parameters.
    params.ordering.geometry.clone()
}
