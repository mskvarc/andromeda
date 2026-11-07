//! The filters a client's query places on registrations beside clause 5.12
//! matching, in the form that selects them (clauses 4.9, 4.10 and 5.10.2.4).
//!
//! Every operation whose selection is a query (Query Entities, the temporal
//! query, EntityMap creation and Purge Entities) carries the same context
//! source filter and geoquery, and a registration is read against them only
//! once they are expanded under the request context. The Scope query holds no
//! term to expand, so it is read from the query as written.

use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine, term_expansion::ContextualExpander};
use andromeda_ngsi_ld_query::{
    error::QueryError,
    filter::{expansion::expand_csf_expr, expression::Expr},
    geo::{admission::GeoQueryAdmission, compile::compile_geo_query, input::GeoQueryInput, query::GeoQuery},
};

/// A query's context source filter and geoquery, expanded under the request
/// context. The default holds neither, which is what a query limited to the
/// local scope places on registrations (clause 5.5.13).
#[derive(Debug, Default)]
pub struct ExpandedRegistrationFilters {
    /// The context source filter, expanded, which a registration's Context
    /// Source Properties have to satisfy (clause 4.9); `None` when the query
    /// has none.
    csf: Option<Expr>,
    /// The geoquery, compiled, which a registration's declared geometry has to
    /// admit (clauses 4.3.6.1 and 5.10.2.4); `None` when the query has none.
    geo: Option<GeoQuery>,
}

impl ExpandedRegistrationFilters {
    /// Compile `geo` under `admission` and expand `csf`, both against
    /// `context`.
    ///
    /// No `jsonKeys` declaration applies to `csf`: Table 5.2.12-1 scopes that
    /// member to Entity attribute names, while `csf` addresses the Context
    /// Source Properties of a registration (clause 4.9), whose values are raw
    /// JSON addressed by their literal keys.
    ///
    /// # Errors
    ///
    /// Returns the [`QueryError`] of compiling `geo`, then that of expanding a
    /// term of `csf`.
    pub async fn expand(
        engine: &JsonLdEngine,
        context: &ContextSpec,
        csf: Option<&Expr>,
        geo: Option<&GeoQueryInput>,
        admission: GeoQueryAdmission,
    ) -> Result<ExpandedRegistrationFilters, QueryError> {
        let prepared_context = engine.prepare_term_context(context);
        let expander = ContextualExpander::new(engine, &prepared_context);
        let geo = compile_geo_query(geo, &expander, admission).await?;
        let csf = match csf {
            // clone: the raw filter stays in the query, which forwards it as the client wrote it
            Some(expression) => Some(expand_csf_expr(expression.clone(), &expander).await?),
            None => None,
        };
        Ok(ExpandedRegistrationFilters { csf, geo })
    }

    /// The expanded context source filter, if the query carries one.
    #[must_use]
    pub const fn csf(&self) -> Option<&Expr> {
        self.csf.as_ref()
    }

    /// The compiled geoquery, if the query carries one.
    #[must_use]
    pub const fn geo(&self) -> Option<&GeoQuery> {
        self.geo.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use crate::{expanded_registration_filters::ExpandedRegistrationFilters, fixed_registration_store::generous_geo_admission};
    use andromeda_json_literal::JsonLiteral;
    use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
    use andromeda_ngsi_ld_query::{filter::parser, geo::input::GeoQueryInput};
    use serde_json::json;

    fn engine() -> JsonLdEngine {
        JsonLdEngine::builder().allow_remote(false).build().expect("engine builds")
    }

    #[tokio::test]
    async fn a_query_without_filters_places_none_on_registrations() {
        let filters = ExpandedRegistrationFilters::expand(&engine(), &ContextSpec::none(), None, None, generous_geo_admission())
            .await
            .expect("nothing to expand");
        assert!(filters.csf().is_none());
        assert!(filters.geo().is_none());
    }

    /// Clause 4.9: the context source filter names Context Source Properties by
    /// the IRI its terms expand to under the request context.
    #[tokio::test]
    async fn the_context_source_filter_is_expanded_under_the_request_context() {
        let context =
            ContextSpec::from_body(JsonLiteral::admit(&json!({"sourceGroup": "https://example.org/sourceGroup"}).to_string()).expect("an admissible literal"));
        let csf = parser::parse("sourceGroup==\"alpha\"").expect("the filter parses");
        let filters = ExpandedRegistrationFilters::expand(&engine(), &context, Some(&csf), None, generous_geo_admission())
            .await
            .expect("the filter expands");
        let expanded = format!("{:?}", filters.csf().expect("the filter is kept"));
        assert!(expanded.contains("https://example.org/sourceGroup"), "{expanded}");
    }

    /// Clause 4.10: the geoquery is compiled with its `geoproperty` defaulting
    /// to `location`.
    #[tokio::test]
    async fn the_geoquery_is_compiled_with_its_default_geoproperty() {
        let input: GeoQueryInput =
            serde_urlencoded::from_str("georel=near%3BmaxDistance%3D%3D1000&geometry=Point&coordinates=%5B-3.7%2C40.4%5D").expect("a decodable geoquery");
        let filters = ExpandedRegistrationFilters::expand(&engine(), &ContextSpec::none(), None, Some(&input), generous_geo_admission())
            .await
            .expect("the geoquery compiles");
        let geo = filters.geo().expect("the geoquery is kept");
        pretty_assertions::assert_eq!(geo.geoproperty.to_string(), "https://uri.etsi.org/ngsi-ld/location");
    }
}
