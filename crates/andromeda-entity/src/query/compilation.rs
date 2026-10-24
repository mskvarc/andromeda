//! Compiling request parameters into a [`CompiledEntityQuery`].
//!
//! [`CompiledEntityQuery`]: andromeda_ngsi_ld_query::entity::compiled::CompiledEntityQuery

use crate::{
    compiled_cache::CompiledQueryKey,
    error::EntityError,
    query::{EntityQueryService, selector_compilation::compile_entity_selectors},
};
use andromeda_jsonld::{context_spec::ContextSpec, engine::term_lookup::PreparedTermContext, term_expansion::ContextualExpander};
use andromeda_ngsi_ld_geometry::geometry_census::GeometryCensusLimits;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::{
    entity::compiled::CompiledEntityQuery,
    entity_selection::{ConsideredEntities, EntitySelection},
    filter::{
        expansion::{apply_expand_values, expand_attr_path, expand_attribute_names, expand_q_expr},
        expression::Expr,
        json_key_attributes::JsonKeyAttributes,
        language_targeting::LanguageTargeting,
        trailing_members::TrailingMembers,
    },
    geo::{compile as geo_compile, query::GeoQuery, reference_budget::admit_reference_geometry},
    ordering::{
        compiled::ResultOrdering,
        geometry::OrderingGeometry,
        term::{CompiledOrderTerm, OrderTarget, RawOrderTarget},
    },
    projection::{
        expansion::{expand_attrs, expand_projection},
        projected_members::ProjectedMembers,
    },
};
use andromeda_ngsi_ld_request::query::entity::parameters::EntityQueryParameters;
use std::{collections::HashSet, sync::Arc};

impl EntityQueryService {
    /// Compile an entity query, its target languages admitted and matched
    /// under `language_targeting`.
    ///
    /// The page the query answers with is no part of the compilation: it is
    /// handed to the store beside the compiled query, so every page of one
    /// query shares one compilation. The level is the request's: the broker's
    /// own unless a context source the request reaches serves a lower one.
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::JsonLd`] when a term of the request cannot be
    /// expanded against `context`, [`EntityError::Query`] when the `q`, `geoQ`
    /// or `orderBy` expression is not well formed or names a target language
    /// `language_targeting` does not admit, and
    /// [`EntityError::Request`] when a query parameter cannot be interpreted.
    pub async fn compile_query_public(
        &self,
        params: &EntityQueryParameters,
        context: &ContextSpec,
        language_targeting: LanguageTargeting,
    ) -> Result<CompiledEntityQuery, EntityError> {
        let compiled = self.compile_query_cached(params, context, language_targeting).await?;
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
        language_targeting: LanguageTargeting,
    ) -> Result<Arc<CompiledEntityQuery>, EntityError> {
        let key = CompiledQueryKey::from_params(params, context, language_targeting);
        self.compiled_cache
            .get_or_compile(key, || self.compile_query(params, context, language_targeting))
            .await
    }

    async fn compile_query(
        &self,
        params: &EntityQueryParameters,
        context: &ContextSpec,
        language_targeting: LanguageTargeting,
    ) -> Result<CompiledEntityQuery, EntityError> {
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
        if let Some(q) = &q_expr {
            language_targeting.admit(q)?;
        }
        let geo = self.compile_geo_query(params, &prepared_context).await?;

        let projection = expand_projection(
            params.pick.as_deref().unwrap_or_default(),
            params.omit.as_deref().unwrap_or_default(),
            &expander,
        )
        .await?;

        let order_by = self.compile_order_terms(params, &prepared_context, language_targeting).await?;
        let ordering_geometry = compile_ordering_geometry(params, self.geo_admission.limits)?;

        let compiled = CompiledEntityQuery::builder()
            .selection(
                EntitySelection::builder()
                    .selectors(selectors)
                    .attrs(attrs)
                    .q_expr(q_expr)
                    .language_matching(language_targeting.matching())
                    .geo(geo)
                    .scope_q(params.scope_q.clone())
                    // Clause 5.5.14: a request narrowed to an empty identifier
                    // set considers no entity, which the selectors' identifier
                    // lists cannot say on their own.
                    .considered(ConsideredEntities::of_request(params.id.as_deref()))
                    // Clause 5.7.2.5: an entity the projection reduces to no
                    // member is not selected, so it is left out before the page.
                    .projected_members(ProjectedMembers::of(&projection))
                    .build(),
            )
            .projection(projection)
            .ordering(ResultOrdering::builder().terms(order_by).geometry(ordering_geometry).build())
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

        let opted_in: HashSet<AttributeName> = match params.expand_values {
            Some(ref raw) => expand_attribute_names(raw, &expander).await?.into_iter().collect(),
            None => HashSet::new(),
        };
        Ok(Some(apply_expand_values(q_expr, &opted_in, &expander).await?))
    }

    /// Compile the `orderBy` terms against the active context (clause 4.23).
    ///
    /// Every name of an ordering path is expanded, not just its head. Clause
    /// 4.9 requires it of the attribute path clause 4.23.3 borrows, "Every name
    /// in the list shall be expanded to a URI (Fully Qualified Name) as mandated
    /// by clause 5.5.7", and the members of a compound value expand for the
    /// same reason, because expansion turns them into IRI-keyed nodes of the
    /// graph. Expanding the head alone addressed `city` in a payload holding
    /// the expansion of `city`, and ordered by nothing.
    ///
    /// The path is expanded under no `jsonKeys` declaration: an attribute whose
    /// value is opaque JSON keeps literal member keys, and the ordering reads
    /// that spelling from the path's own pre-expansion copy rather than from a
    /// declaration `orderBy` has no parameter for.
    ///
    /// Clause 4.23.3 borrows the clause 4.9 path, so a term's target language
    /// is held to `language_targeting` as a `q` one is.
    async fn compile_order_terms(
        &self,
        params: &EntityQueryParameters,
        context: &PreparedTermContext,
        language_targeting: LanguageTargeting,
    ) -> Result<Vec<CompiledOrderTerm>, EntityError> {
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
                // The system timestamps read their own columns and carry no IRI
                // to expand, so they pass through like `id`/`type`/`scope`.
                RawOrderTarget::CreatedAt => OrderTarget::CreatedAt,
                RawOrderTarget::ModifiedAt => OrderTarget::ModifiedAt,
                // clone: expansion rewrites the path it consumes, and the cached
                // request parameters stay immutable across compilations.
                RawOrderTarget::Attribute(path) => {
                    language_targeting.admit_path(path)?;
                    OrderTarget::Attribute(expand_attr_path(path.clone(), TrailingMembers::Terms(&json_keys), &expander).await?)
                }
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
    ///
    /// # Errors
    ///
    /// Returns the query error of the compilation, and the `TooComplexQuery`
    /// of clause 5.5.6 for a reference geometry larger than the broker
    /// resolves.
    async fn compile_geo_query(&self, params: &EntityQueryParameters, context: &PreparedTermContext) -> Result<Option<GeoQuery>, EntityError> {
        Ok(geo_compile::compile_geo_query(params.geo.0.as_ref(), &ContextualExpander::new(self.engine(), context), self.geo_admission).await?)
    }
}

/// Compile `orderFrom` and `orderGeometry` into the reference geometry a
/// distance ordering measures from (clause 5.2.43).
///
/// # Errors
///
/// Returns the `TooComplexQuery` of clause 5.5.6 for a reference geometry
/// holding more positions than `limits` admits.
fn compile_ordering_geometry(params: &EntityQueryParameters, limits: GeometryCensusLimits) -> Result<Option<OrderingGeometry>, EntityError> {
    let Some(ordering) = &params.ordering.geometry else {
        return Ok(None);
    };
    admit_reference_geometry(&ordering.geometry, limits)?;
    // The compiled query owns the geometry independently of the reusable request parameters.
    Ok(Some(ordering.clone()))
}

#[cfg(test)]
mod tests {
    use crate::{error::EntityError, query::service_test_double::service_resolving};
    use andromeda_jsonld::context_spec::ContextSpec;
    use andromeda_ngsi_ld_geometry::geometry_census::{GeometryCensusLimits, GeometryPositionLimit};
    use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
    use andromeda_ngsi_ld_query::{error::QueryError, filter::language_targeting::LanguageTargeting};
    use andromeda_ngsi_ld_request::query::entity::parameters::EntityQueryParameters;
    use std::num::NonZeroUsize;

    /// A query for the Vehicles within a square: one ring of five positions.
    const WITHIN_A_SQUARE: &str =
        "type=Vehicle&georel=within&geometry=Polygon&coordinates=%5B%5B%5B0%2C0%5D%2C%5B1%2C0%5D%2C%5B1%2C1%5D%2C%5B0%2C1%5D%2C%5B0%2C0%5D%5D%5D";
    /// A query for the Vehicles ordered by their distance from a line of three
    /// positions (clause 5.2.43).
    const ORDERED_FROM_A_LINE: &str = "type=Vehicle&orderFrom=%5B%5B2.3%2C48.8%5D%2C%5B2.4%2C48.9%5D%2C%5B2.5%2C49.0%5D%5D&orderGeometry=LineString";

    fn params(query: &str) -> EntityQueryParameters {
        serde_urlencoded::from_str(query).expect("a valid query string")
    }

    fn limit(positions: usize) -> GeometryCensusLimits {
        GeometryCensusLimits {
            positions: GeometryPositionLimit::new(NonZeroUsize::new(positions).expect("the fixture limit admits a position")),
            values: JsonValueLimit::new(NonZeroUsize::MAX),
        }
    }

    /// Compile `query` against a broker resolving reference geometries of at
    /// most `positions` positions.
    async fn compiled(query: &str, positions: usize) -> Result<(), EntityError> {
        service_resolving(limit(positions))
            .compile_query_public(&params(query), &ContextSpec::core_only(), LanguageTargeting::Specification)
            .await
            .map(drop)
    }

    /// Clause 5.5.6 raises `TooComplexQuery` for a query the broker cannot
    /// resolve, which is what the error names.
    fn assert_too_complex(result: Result<(), EntityError>) {
        let error = result.expect_err("the reference geometry exceeds the budget");
        assert!(matches!(error, EntityError::Query(QueryError::ReferenceGeometryTooComplex { .. })), "{error:?}");
    }

    #[tokio::test]
    async fn a_geo_query_within_the_position_budget_compiles() {
        compiled(WITHIN_A_SQUARE, 5).await.expect("five positions are within a budget of five");
    }

    #[tokio::test]
    async fn a_geo_query_past_the_position_budget_is_too_complex() {
        assert_too_complex(compiled(WITHIN_A_SQUARE, 4).await);
    }

    #[tokio::test]
    async fn an_order_from_geometry_past_the_budget_is_too_complex() {
        compiled(ORDERED_FROM_A_LINE, 3).await.expect("three positions are within a budget of three");
        assert_too_complex(compiled(ORDERED_FROM_A_LINE, 2).await);
    }
}
