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
        expansion::{apply_expand_values, expand_attr_path, expand_attribute_names, expand_json_keys, expand_q_expr},
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
        attrs_selection::{AttrsAdmission, expand_attrs_admitted},
        expansion::expand_projection,
        projected_members::ProjectedMembers,
    },
    term_expander::TermExpander,
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
    /// Reports [`EntityError::JsonLd`] when `context` does not resolve or
    /// process, or a term of the request cannot be expanded against it,
    /// [`EntityError::Query`] when the `q`, `geoQ`
    /// or `orderBy` expression is not well formed, names a target language
    /// `language_targeting` does not admit, or is too complex to resolve
    /// (clause 5.5.6), and
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
    /// Resolves `context` first, so a compiled query is reused only while the
    /// documents it resolved to are still the ones the loader holds, and a
    /// context that no longer resolves fails here as it would on a first
    /// compilation. The cache is keyed on the request parameters and that
    /// processed context; on a miss, the query is compiled and stored.
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::JsonLd`] when `context` does not resolve or
    /// process, and every error [`EntityQueryService::compile_query_public`]
    /// documents on a miss.
    pub(super) async fn compile_query_cached(
        &self,
        params: &EntityQueryParameters,
        context: &ContextSpec,
        language_targeting: LanguageTargeting,
    ) -> Result<Arc<CompiledEntityQuery>, EntityError> {
        let prepared_context = self.engine().prepare_term_context(context);
        let processed = prepared_context.expansion(self.engine()).await?.id();
        let key = CompiledQueryKey::from_params(params, processed, language_targeting);
        self.compiled_cache
            .get_or_compile(key, || self.compile_query(params, &prepared_context, language_targeting))
            .await
    }

    /// Compile `params` against `prepared_context` without consulting the
    /// cache, admitting the `q` filter's complexity on the way.
    async fn compile_query(
        &self,
        params: &EntityQueryParameters,
        prepared_context: &PreparedTermContext,
        language_targeting: LanguageTargeting,
    ) -> Result<CompiledEntityQuery, EntityError> {
        let selectors = compile_entity_selectors(self.engine(), prepared_context, &params.selectors()).await?;

        let expander = ContextualExpander::new(self.engine(), prepared_context);
        // Table 6.4.3.2-1 restricts `attrs` to "an Attribute (Property or
        // Relationship) name". A query answers in an Entity representation, where
        // clause 4.18 leaves the Scope non-reified, so `scope` names nothing the
        // row admits.
        let attrs = expand_attrs_admitted(params.attrs.as_deref().unwrap_or_default(), AttrsAdmission::AttributeNamesOnly, &expander).await?;
        let json_keys = expand_json_keys(params.json_keys.as_deref(), &expander).await?;
        if let Some(q) = &params.q {
            self.filter_complexity.admit(q)?;
        }
        let q_expr = compile_q_expression(params, &json_keys, &expander).await?;
        if let Some(q) = &q_expr {
            language_targeting.admit(q)?;
        }
        let geo = self.compile_geo_query(params, prepared_context).await?;

        let projection = expand_projection(
            params.pick.as_deref().unwrap_or_default(),
            params.omit.as_deref().unwrap_or_default(),
            &expander,
        )
        .await?;

        let order_by = compile_order_terms(params, &json_keys, &expander, language_targeting).await?;
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

/// Expand the `q` filter under the request's clause 4.9 `jsonKeys`
/// declaration, then apply the `expandValues` coercion to the result.
async fn compile_q_expression(
    params: &EntityQueryParameters,
    json_keys: &JsonKeyAttributes,
    expander: &impl TermExpander,
) -> Result<Option<Expr>, EntityError> {
    // Expansion consumes its AST; the cache-owned request remains immutable across lookups.
    let Some(q_expr) = params.q.clone() else {
        return Ok(None);
    };
    let q_expr = expand_q_expr(q_expr, json_keys, expander).await?;

    let opted_in: HashSet<AttributeName> = match params.expand_values {
        Some(ref raw) => expand_attribute_names(raw, expander).await?.into_iter().collect(),
        None => HashSet::new(),
    };
    Ok(Some(apply_expand_values(q_expr, &opted_in, expander).await?))
}

/// Compile the `orderBy` terms against the active context (clause 4.23).
///
/// Every name of an ordering path is expanded, not just its head. Clause 4.9
/// requires it of the attribute path clause 4.23.3 borrows, "Every name in the
/// list shall be expanded to a URI (Fully Qualified Name) as mandated by clause
/// 5.5.7", and the members of a compound value expand for the same reason,
/// because expansion turns them into IRI-keyed nodes of the graph.
///
/// The members under an attribute the request declared in `jsonKeys` stay
/// literal object keys (clause 4.9 EXAMPLE 11), so an ordering path resolves
/// as a `q` path into the same attribute does. Clause 4.23.3 borrows the
/// clause 4.9 path, so a term's target language is held to
/// `language_targeting` as a `q` one is.
async fn compile_order_terms(
    params: &EntityQueryParameters,
    json_keys: &JsonKeyAttributes,
    expander: &impl TermExpander,
    language_targeting: LanguageTargeting,
) -> Result<Vec<CompiledOrderTerm>, EntityError> {
    // Parsed and validated at the query boundary (clause 4.23).
    let collation = params.ordering.collation.clone();
    let mut compiled = Vec::new();
    for term in &params.ordering.order_by {
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
                OrderTarget::Attribute(expand_attr_path(path.clone(), TrailingMembers::Terms(json_keys), expander).await?)
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
    use crate::{
        error::EntityError,
        query::{
            EntityQueryService,
            service_test_double::{service_expanding_through, service_resolving},
        },
    };
    use andromeda_json_literal::JsonLiteral;
    use andromeda_jsonld::{
        context_cache::{CachedContext, ContextCache, ContextCacheError, StoredDocumentSource},
        context_spec::ContextSpec,
        engine::processor::JsonLdEngine,
        loader::cache_lifetime::{CacheLifetime, ResidualLifetime},
    };
    use andromeda_ngsi_ld_geometry::geometry_census::{GeometryCensusLimits, GeometryPositionLimit};
    use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
    use andromeda_ngsi_ld_query::{
        entity::compiled::CompiledEntityQuery,
        error::QueryError,
        filter::language_targeting::LanguageTargeting,
        ordering::term::OrderTarget,
    };
    use andromeda_ngsi_ld_request::query::entity::parameters::EntityQueryParameters;
    use iri_rs::IriBuf;
    use serde_json::{Value, json};
    use std::{
        num::NonZeroUsize,
        sync::{Arc, RwLock},
    };

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

    /// The member names the first `orderBy` term of `query` addresses once
    /// compiled under the core context.
    async fn ordered_members(query: &str) -> Vec<String> {
        let compiled = service_resolving(limit(5))
            .compile_query_public(&params(query), &ContextSpec::core_only(), LanguageTargeting::Specification)
            .await
            .expect("the query compiles");
        let path = compiled
            .ordering
            .terms
            .first()
            .and_then(|term| if let OrderTarget::Attribute(path) = &term.target { Some(path) } else { None })
            .expect("the first term orders by an attribute");
        path.members()
            .expect("the path addresses a member")
            .addressed
            .iter()
            .map(|name| name.as_str().to_owned())
            .collect()
    }

    /// Clause 4.9 EXAMPLE 11: under a `jsonKeys` attribute `value` is the
    /// literal object key, not the core context's `ngsi-ld:hasValue`, for an
    /// `orderBy` path as for a `q` one.
    #[tokio::test]
    async fn an_order_by_member_under_a_declared_json_key_stays_the_literal_key() {
        pretty_assertions::assert_eq!(
            ordered_members("type=Vehicle&orderBy=parkingTickets%5Bvalue%5D&jsonKeys=parkingTickets").await,
            vec!["value"]
        );
    }

    #[tokio::test]
    async fn an_order_by_member_under_an_undeclared_attribute_expands() {
        pretty_assertions::assert_eq!(
            ordered_members("type=Vehicle&orderBy=parkingTickets%5Bvalue%5D&jsonKeys=other").await,
            vec!["https://uri.etsi.org/ngsi-ld/hasValue"]
        );
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

    /// Clause 5.5.6: a `q` making more comparisons, or nesting deeper, than
    /// the broker's default limits (1000 and 16) is `TooComplexQuery`; one at
    /// them compiles.
    #[tokio::test]
    async fn a_q_past_the_complexity_limits_is_too_complex_and_one_at_them_compiles() {
        let values = |count: usize| (0..count).map(|value| value.to_string()).collect::<Vec<_>>().join("%2C");
        let nested = |levels: usize| (1..levels).fold("a%3E1%3Bb%3E1".to_owned(), |inner, level| format!("c{level}%3E1%7C%28{inner}%29"));
        for (q, admitted) in [
            (format!("speed%3D%3D{}", values(1000)), true),
            (format!("speed%3D%3D{}", values(1001)), false),
            (nested(16), true),
            (nested(17), false),
        ] {
            let result = compiled(&format!("type=Vehicle&q={q}"), 5).await;
            if admitted {
                assert!(result.is_ok(), "{q}: {result:?}");
            } else {
                assert!(
                    matches!(
                        result,
                        Err(EntityError::Query(QueryError::QTooManyComparisons { .. } | QueryError::QNestedTooDeep { .. }))
                    ),
                    "{q}: {result:?}"
                );
            }
        }
    }

    /// A stored `@context` row a test replaces or deletes, as Delete @context
    /// does with and without reload (clause 5.13.5.4).
    #[derive(Debug)]
    struct StoredRow {
        /// The document the row holds, or `None` once it is deleted.
        payload: RwLock<Option<JsonLiteral>>,
    }

    impl StoredRow {
        /// A row holding `document`.
        fn holding(document: &Value) -> Arc<StoredRow> {
            Arc::new(StoredRow {
                payload: RwLock::new(Some(stored_literal(document))),
            })
        }

        /// Replace the document the row holds.
        fn replace(&self, document: &Value) {
            *self.payload.write().expect("the row lock") = Some(stored_literal(document));
        }

        /// Delete the row.
        fn delete(&self) {
            *self.payload.write().expect("the row lock") = None;
        }
    }

    #[async_trait::async_trait]
    impl ContextCache for StoredRow {
        async fn get(&self, _url: &IriBuf) -> Result<Option<CachedContext>, ContextCacheError> {
            Ok(self.payload.read().expect("the row lock").clone().map(|payload| CachedContext {
                payload,
                residual: ResidualLifetime::Unbounded,
                canonical_url: None,
                source: StoredDocumentSource::Broker,
            }))
        }

        async fn put(&self, _url: &IriBuf, _document_url: &IriBuf, _payload: &JsonLiteral, _lifetime: CacheLifetime) -> Result<(), ContextCacheError> {
            Ok(())
        }
    }

    fn stored_literal(document: &Value) -> JsonLiteral {
        JsonLiteral::admit(&document.to_string()).expect("an admissible document")
    }

    /// The `@context` the stored row is read under.
    fn stored_url() -> IriBuf {
        IriBuf::new("https://example.org/stored.jsonld".to_owned()).expect("a valid IRI")
    }

    /// A query service whose engine resolves [`stored_url`] from `row` alone.
    fn service_reading(row: &Arc<StoredRow>) -> EntityQueryService {
        service_expanding_through(
            JsonLdEngine::builder()
                .allow_remote(false)
                .with_l2_cache(Arc::clone(row) as Arc<dyn ContextCache>)
                .build()
                .expect("engine builds"),
        )
    }

    /// The same query, compiled through `service`'s cache under a `Link` to
    /// [`stored_url`].
    async fn compiled_under_the_stored_context(service: &EntityQueryService) -> Result<Arc<CompiledEntityQuery>, EntityError> {
        service
            .compile_query_cached(
                &params("type=Vehicle&attrs=speed"),
                &ContextSpec::from_link(stored_url()),
                LanguageTargeting::Specification,
            )
            .await
    }

    fn attrs(compiled: &CompiledEntityQuery) -> Vec<&str> {
        compiled.selection.attrs.iter().map(IriBuf::as_str).collect()
    }

    #[tokio::test]
    async fn a_query_repeated_under_an_unchanged_context_reuses_its_compilation() {
        let row = StoredRow::holding(&json!({ "@context": { "speed": "https://example.org/speed" } }));
        let service = service_reading(&row);

        let first = compiled_under_the_stored_context(&service).await.expect("the query compiles");
        let second = compiled_under_the_stored_context(&service).await.expect("the query compiles");

        assert!(Arc::ptr_eq(&first, &second), "the second query recompiled");
    }

    /// Clause 5.13.5.4 replaces a reloaded `@context` with the document
    /// downloaded then, so the repeated query expands `speed` by that document.
    #[tokio::test]
    async fn a_query_repeated_after_its_context_is_reloaded_expands_by_the_new_document() {
        let row = StoredRow::holding(&json!({ "@context": { "speed": "https://example.org/speed" } }));
        let service = service_reading(&row);
        let first = compiled_under_the_stored_context(&service).await.expect("the query compiles");

        row.replace(&json!({ "@context": { "speed": "https://example.org/velocity" } }));
        service.engine().invalidate_context(&stored_url()).await;
        let second = compiled_under_the_stored_context(&service).await.expect("the query compiles");

        pretty_assertions::assert_eq!(attrs(&first), vec!["https://example.org/speed"]);
        pretty_assertions::assert_eq!(attrs(&second), vec!["https://example.org/velocity"]);
    }

    #[tokio::test]
    async fn a_query_repeated_after_its_context_is_deleted_names_an_unresolvable_context() {
        let row = StoredRow::holding(&json!({ "@context": { "speed": "https://example.org/speed" } }));
        let service = service_reading(&row);
        compiled_under_the_stored_context(&service).await.expect("the query compiles");

        row.delete();
        service.engine().invalidate_context(&stored_url()).await;
        let error = compiled_under_the_stored_context(&service).await.expect_err("the context no longer resolves");

        assert!(
            matches!(&error, EntityError::JsonLd(failure) if failure.is_context_not_available()),
            "{error:?}"
        );
    }
}
