//! Spelling the names a forwarded request carries in its forwarding context
//! (clause 4.3.6.6).
//!
//! Clause 4.3.6.6 compacts a forwarded request "over both payload and query
//! parameters using the JSON-LD Context supplied" when a registration
//! configures one, and a request whose inbound context no single `Link` header
//! can name is written against the core context
//! ([`crate::forwarding_context`]). Either way the names a source receives are
//! not the client's spelling: each is expanded against the inbound context and
//! compacted against the forwarding one. When the forwarding context is the
//! inbound one every name travels as the client wrote it.
//!
//! One [`ForwardedNames`] is built per registration, because the forwarding
//! context is a property of the registration; the expansion side is the
//! operation's [`TermResolver`], shared by every registration.

use crate::{error::FederationError, federation_invariant::FederationInvariant, forwarding_context::Rewording, term_resolver::TermResolver};
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine, term_expansion::ContextualExpander};
use andromeda_ngsi_ld_compaction::name_compaction::NameCompaction;
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    entity_type_name::EntityTypeName,
    reference::{NgsiLdReference, NgsiLdReferenceError},
};
use andromeda_ngsi_ld_query::{
    filter::{
        compaction::{compact_csf_expr, compact_q_expr},
        expansion::{expand_csf_expr, expand_q_expr},
        expression::Expr,
        json_key_attributes::JsonKeyAttributes,
    },
    projection::grammar::RawProjectionTerm,
    type_selection::grammar::TypeExprRaw,
};
use std::{future::Future, pin::Pin, str::FromStr};

/// The names one forwarded request carries, spelled in its forwarding context.
pub(crate) struct ForwardedNames<'a> {
    /// Expands the client's names against the inbound context.
    resolver: &'a TermResolver<'a>,
    /// The engine the inbound expander reads.
    engine: &'a JsonLdEngine,
    /// Compacts names against the forwarding context.
    names: NameCompaction,
    /// Whether the forwarding context differs from the inbound one.
    rewording: Rewording,
}

impl<'a> ForwardedNames<'a> {
    /// The names a request forwarded under `naming` carries, reworded from the
    /// inbound context when `rewording` says the two differ.
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::JsonLd`] when `naming` does not resolve or
    /// does not process.
    pub(crate) async fn prepare(
        engine: &'a JsonLdEngine,
        resolver: &'a TermResolver<'a>,
        naming: &ContextSpec,
        rewording: Rewording,
    ) -> Result<ForwardedNames<'a>, FederationError> {
        Ok(ForwardedNames {
            resolver,
            engine,
            names: NameCompaction::new(engine.compactor(naming).processed_context().await?),
            rewording,
        })
    }

    /// The inbound-context expansion this spelling is built on.
    pub(crate) const fn resolver(&self) -> &'a TermResolver<'a> {
        self.resolver
    }

    /// Whether the names are reworded from the inbound context.
    pub(crate) const fn rewording(&self) -> Rewording {
        self.rewording
    }

    /// A registered attribute IRI spelled in the forwarding context, or the IRI
    /// itself when the context abbreviates none (clause 4.5.1).
    ///
    /// # Errors
    ///
    /// Returns [`FederationInvariant::ForwardedName`] when the compacted
    /// spelling is not an NGSI-LD name.
    pub(crate) fn compact_attr(&self, iri: &AttributeName) -> Result<AttributeName, FederationError> {
        forwarded_name(&self.names.compact_attr_iri(iri.as_str()))
    }

    /// A registered entity type IRI spelled in the forwarding context, or the
    /// IRI itself when the context abbreviates none (clause 4.5.1).
    ///
    /// # Errors
    ///
    /// Returns [`FederationInvariant::ForwardedName`] when the compacted
    /// spelling is not an NGSI-LD name.
    pub(crate) fn compact_type(&self, iri: &EntityTypeName) -> Result<EntityTypeName, FederationError> {
        forwarded_name(&self.names.compact_type_iri(iri.as_str()))
    }

    /// A client's attribute name, spelled in the forwarding context.
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::JsonLd`] when the name does not expand
    /// against the inbound context, and [`FederationInvariant::ForwardedName`]
    /// when its compacted spelling is not an NGSI-LD name.
    pub(crate) async fn attr(&self, term: &AttributeName) -> Result<AttributeName, FederationError> {
        match self.rewording {
            Rewording::AsInbound => Ok(term.clone()),
            Rewording::Reword => {
                let iri = self.resolver.expand_attr(term.as_str()).await?;
                forwarded_name(&self.names.compact_attr_iri(iri.as_str()))
            }
        }
    }

    /// A client's entity type name, spelled in the forwarding context.
    ///
    /// # Errors
    ///
    /// As [`ForwardedNames::attr`].
    pub(crate) async fn entity_type(&self, term: &EntityTypeName) -> Result<EntityTypeName, FederationError> {
        match self.rewording {
            Rewording::AsInbound => Ok(term.clone()),
            Rewording::Reword => {
                let iri = self.resolver.expand_type(term.as_str()).await?;
                forwarded_name(&self.names.compact_type_iri(iri.as_str()))
            }
        }
    }

    /// A client's clause 4.17 type selection with every name spelled in the
    /// forwarding context, its structure unchanged.
    ///
    /// # Errors
    ///
    /// As [`ForwardedNames::attr`].
    pub(crate) async fn type_expr(&self, expression: &TypeExprRaw) -> Result<TypeExprRaw, FederationError> {
        match self.rewording {
            Rewording::AsInbound => Ok(expression.clone()),
            Rewording::Reword => self.reworded_type_expr(expression).await,
        }
    }

    /// Reword every leaf of a type selection.
    fn reworded_type_expr<'b>(&'b self, expression: &'b TypeExprRaw) -> Pin<Box<dyn Future<Output = Result<TypeExprRaw, FederationError>> + Send + 'b>> {
        Box::pin(async move {
            match expression {
                TypeExprRaw::Wildcard => Ok(TypeExprRaw::Wildcard),
                TypeExprRaw::Type(name) => Ok(TypeExprRaw::Type(self.entity_type(name).await?)),
                TypeExprRaw::And(children) => Ok(TypeExprRaw::And(self.reworded_children(children).await?)),
                TypeExprRaw::Or(children) => Ok(TypeExprRaw::Or(self.reworded_children(children).await?)),
            }
        })
    }

    /// Reword the children of one boolean type group, in order.
    async fn reworded_children(&self, children: &[TypeExprRaw]) -> Result<Vec<TypeExprRaw>, FederationError> {
        let mut reworded = Vec::with_capacity(children.len());
        for child in children {
            reworded.push(self.reworded_type_expr(child).await?);
        }
        Ok(reworded)
    }

    /// A client's clause 4.9 query with every attribute, sub-attribute,
    /// compound-value member, Relationship and guard type spelled in the
    /// forwarding context; operators and operands unchanged. `json_keys` names
    /// the attributes whose members are literal JSON keys (clause 4.9 EXAMPLE
    /// 11), in the client's spelling.
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::Query`] when a name in the query does not
    /// expand against the inbound context, and [`FederationError::JsonLd`] when
    /// a `jsonKeys` name does not.
    pub(crate) async fn q(&self, expression: &Expr, json_keys: Option<&[AttributeName]>) -> Result<Expr, FederationError> {
        match self.rewording {
            Rewording::AsInbound => Ok(expression.clone()),
            Rewording::Reword => {
                let mut literal_keys = Vec::new();
                for name in json_keys.unwrap_or_default() {
                    literal_keys.push(forwarded_name(self.resolver.expand_attr(name.as_str()).await?.as_str())?);
                }
                let literal_keys: JsonKeyAttributes = literal_keys.into_iter().collect();
                let inbound = ContextualExpander::new(self.engine, self.resolver.prepared());
                let expanded = expand_q_expr(expression.clone(), &literal_keys, &inbound).await?; // clone: the walker consumes the expression the client's parameters keep
                Ok(compact_q_expr(expanded, &self.names).await)
            }
        }
    }

    /// A client's clause 4.9 Context Source filter with every property and
    /// sub-property spelled in the forwarding context, and its compound-value
    /// members as the client wrote them: they are the raw JSON keys of a
    /// Context Source Property, which no `@context` rewrites.
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::Query`] when a name in the filter does not
    /// expand against the inbound context.
    pub(crate) async fn csf(&self, expression: &Expr) -> Result<Expr, FederationError> {
        match self.rewording {
            Rewording::AsInbound => Ok(expression.clone()),
            Rewording::Reword => {
                let inbound = ContextualExpander::new(self.engine, self.resolver.prepared());
                let expanded = expand_csf_expr(expression.clone(), &inbound).await?; // clone: the walker consumes the expression the client's parameters keep
                Ok(compact_csf_expr(expanded, &self.names).await)
            }
        }
    }

    /// A client's clause 4.21 projection with every name spelled in the
    /// forwarding context.
    ///
    /// # Errors
    ///
    /// As [`ForwardedNames::attr`].
    pub(crate) async fn projection(&self, terms: &[RawProjectionTerm]) -> Result<Vec<RawProjectionTerm>, FederationError> {
        let mut reworded = Vec::with_capacity(terms.len());
        for term in terms {
            reworded.push(self.reworded_projection_term(term).await?);
        }
        Ok(reworded)
    }

    /// Reword one projection term and its linked-entity sub-terms.
    fn reworded_projection_term<'b>(
        &'b self,
        term: &'b RawProjectionTerm,
    ) -> Pin<Box<dyn Future<Output = Result<RawProjectionTerm, FederationError>> + Send + 'b>> {
        Box::pin(async move {
            Ok(RawProjectionTerm {
                name: self.attr(&term.name).await?,
                linked: self.projection(&term.linked).await?,
            })
        })
    }

    /// A client's `GeoProperty` name, spelled in the forwarding context.
    ///
    /// # Errors
    ///
    /// As [`ForwardedNames::attr`].
    pub(crate) async fn geoproperty(&self, name: &NgsiLdReference) -> Result<NgsiLdReference, FederationError> {
        match self.rewording {
            Rewording::AsInbound => Ok(name.clone()),
            Rewording::Reword => {
                let iri = self.resolver.expand_attr(name.as_str()).await?;
                forwarded_name(&self.names.compact_attr_iri(iri.as_str()))
            }
        }
    }
}

/// A compacted spelling as the NGSI-LD name it has to be.
///
/// Compaction answers a term or a full IRI, both of which the clause 4.6.2
/// name grammar accepts, so a refusal here means the forwarding context maps a
/// name to text no NGSI-LD name can be: a broken context, not a value to
/// forward as written.
fn forwarded_name<N>(spelling: &str) -> Result<N, FederationError>
where
    N: FromStr<Err = NgsiLdReferenceError>,
{
    spelling
        .parse()
        .map_err(|error| FederationError::from(FederationInvariant::ForwardedName(error)))
}

#[cfg(test)]
mod tests {
    use crate::{forwarded_names::ForwardedNames, forwarding_context::Rewording, term_resolver::TermResolver};
    use andromeda_json_literal::JsonLiteral;
    use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
    use andromeda_ngsi_ld_query::{filter::parser, type_selection::grammar::TypeExprRaw};
    use serde_json::json;
    use url::form_urlencoded;

    /// An engine that resolves only what is embedded, never the network.
    fn engine() -> JsonLdEngine {
        JsonLdEngine::builder().allow_remote(false).build().expect("engine builds")
    }

    /// The client's context: `speed` and `Vehicle` alias IRIs the forwarding
    /// context spells differently.
    fn inbound() -> ContextSpec {
        ContextSpec::from_body(
            JsonLiteral::admit(
                &json!({
                    "speed": "https://example.org/speed",
                    "Vehicle": "https://example.org/Vehicle",
                    "brand": "https://example.org/brand"
                })
                .to_string(),
            )
            .expect("an admissible literal"),
        )
    }

    /// The forwarding context: the same IRIs under other terms.
    fn configured() -> ContextSpec {
        ContextSpec::from_body(
            JsonLiteral::admit(
                &json!({
                    "velocity": "https://example.org/speed",
                    "Car": "https://example.org/Vehicle"
                })
                .to_string(),
            )
            .expect("an admissible literal"),
        )
    }

    fn attr(name: &str) -> AttributeName {
        name.parse().expect("an attribute name")
    }

    #[tokio::test]
    async fn names_are_reworded_from_the_inbound_to_the_forwarding_context() {
        let engine = engine();
        let inbound = inbound();
        let naming = configured();
        let resolver = TermResolver::new(&engine, &inbound);
        let names = ForwardedNames::prepare(&engine, &resolver, &naming, Rewording::Reword)
            .await
            .expect("the forwarding context processes");

        pretty_assertions::assert_eq!(names.attr(&attr("speed")).await.expect("rewords").as_str(), "velocity");
        pretty_assertions::assert_eq!(names.attr(&attr("brand")).await.expect("rewords").as_str(), "https://example.org/brand");
        let vehicle: EntityTypeName = "Vehicle".parse().expect("a type name");
        pretty_assertions::assert_eq!(names.entity_type(&vehicle).await.expect("rewords").as_str(), "Car");
        let expression: TypeExprRaw = "Vehicle,Other".parse().expect("a type selection");
        pretty_assertions::assert_eq!(names.type_expr(&expression).await.expect("rewords").to_string(), "Car,Other");
        let query = parser::parse("speed>10;brand==\"x\"").expect("a query");
        pretty_assertions::assert_eq!(
            names.q(&query, None).await.expect("rewords").to_string(),
            "velocity>10;\"https://example.org/brand\"==\"x\""
        );
    }

    #[tokio::test]
    async fn names_travel_unchanged_when_the_forwarding_context_is_the_inbound_one() {
        let engine = engine();
        let inbound = inbound();
        let resolver = TermResolver::new(&engine, &inbound);
        let names = ForwardedNames::prepare(&engine, &resolver, &inbound, Rewording::AsInbound)
            .await
            .expect("the forwarding context processes");
        pretty_assertions::assert_eq!(names.attr(&attr("speed")).await.expect("keeps").as_str(), "speed");
        pretty_assertions::assert_eq!(
            names.query_string("type=Vehicle&attrs=speed&q=speed%3E10").await.expect("keeps"),
            "type=Vehicle&attrs=speed&q=speed%3E10"
        );
    }

    #[tokio::test]
    async fn a_raw_query_string_rewords_only_its_name_bearing_parameters() {
        let engine = engine();
        let inbound = inbound();
        let naming = configured();
        let resolver = TermResolver::new(&engine, &inbound);
        let names = ForwardedNames::prepare(&engine, &resolver, &naming, Rewording::Reword)
            .await
            .expect("the forwarding context processes");
        let reworded = names
            .query_string("type=Vehicle&attrs=speed,brand&q=speed%3E10&pick=speed&geoproperty=speed&limit=5&id=urn:ngsi-ld:Vehicle:1")
            .await
            .expect("rewords");
        let pairs = form_urlencoded::parse(reworded.as_bytes()).into_owned().collect::<Vec<_>>();
        let value = |key: &str| pairs.iter().find(|(name, _)| name == key).map(|(_, value)| value.as_str());
        pretty_assertions::assert_eq!(value("type"), Some("Car"));
        pretty_assertions::assert_eq!(value("attrs"), Some("velocity,https://example.org/brand"));
        pretty_assertions::assert_eq!(value("q"), Some("velocity>10"));
        pretty_assertions::assert_eq!(value("pick"), Some("velocity"));
        pretty_assertions::assert_eq!(value("geoproperty"), Some("velocity"));
        pretty_assertions::assert_eq!(value("limit"), Some("5"));
        pretty_assertions::assert_eq!(value("id"), Some("urn:ngsi-ld:Vehicle:1"));
    }

    /// Clause 4.9 EXAMPLE 11: the members of an attribute `jsonKeys` names are
    /// literal JSON keys, so the query keeps them as written while the
    /// attribute itself, and the `jsonKeys` list, are spelled in the forwarding
    /// context.
    #[tokio::test]
    async fn json_keys_members_stay_literal_while_their_attribute_is_reworded() {
        let engine = engine();
        let inbound = ContextSpec::from_body(
            JsonLiteral::admit(
                &json!({
                    "speed": "https://example.org/speed",
                    "road": "https://example.org/street"
                })
                .to_string(),
            )
            .expect("an admissible literal"),
        );
        let naming = ContextSpec::from_body(
            JsonLiteral::admit(
                &json!({
                    "velocity": "https://example.org/speed",
                    "street": "https://example.org/street"
                })
                .to_string(),
            )
            .expect("an admissible literal"),
        );
        let resolver = TermResolver::new(&engine, &inbound);
        let names = ForwardedNames::prepare(&engine, &resolver, &naming, Rewording::Reword)
            .await
            .expect("the forwarding context processes");

        let literal = names.query_string("q=speed%5Broad%5D%3D%3D%22Main%22&jsonKeys=speed").await.expect("rewords");
        let pairs = form_urlencoded::parse(literal.as_bytes()).into_owned().collect::<Vec<_>>();
        let value = |key: &str| pairs.iter().find(|(name, _)| name == key).map(|(_, value)| value.clone());
        pretty_assertions::assert_eq!(value("q").as_deref(), Some("velocity[road]==\"Main\""));
        pretty_assertions::assert_eq!(value("jsonKeys").as_deref(), Some("velocity"));

        let expanded = names.query_string("q=speed%5Broad%5D%3D%3D%22Main%22").await.expect("rewords");
        let pairs = form_urlencoded::parse(expanded.as_bytes()).into_owned().collect::<Vec<_>>();
        pretty_assertions::assert_eq!(
            pairs.iter().find(|(name, _)| name == "q").map(|(_, value)| value.as_str()),
            Some("velocity[street]==\"Main\"")
        );
    }

    /// A context source filter names Context Source Properties and is spelled
    /// like a query; a parameter the binding tables do not name travels as
    /// written.
    #[tokio::test]
    async fn a_source_filter_is_reworded_and_an_unknown_parameter_travels_as_written() {
        let engine = engine();
        let inbound = inbound();
        let naming = configured();
        let resolver = TermResolver::new(&engine, &inbound);
        let names = ForwardedNames::prepare(&engine, &resolver, &naming, Rewording::Reword)
            .await
            .expect("the forwarding context processes");
        let reworded = names.query_string("csf=speed%3D%3D1&orderBy=speed").await.expect("rewords");
        let pairs = form_urlencoded::parse(reworded.as_bytes()).into_owned().collect::<Vec<_>>();
        pretty_assertions::assert_eq!(
            pairs,
            vec![("csf".to_owned(), "velocity==1".to_owned()), ("orderBy".to_owned(), "speed".to_owned())]
        );
    }

    /// Clause 4.9: a context source filter's trailing path addresses the raw
    /// JSON keys of a Context Source Property, so the member travels as the
    /// client wrote it even where the contexts would reword it as a term.
    #[tokio::test]
    async fn a_source_filter_keeps_its_compound_value_members_as_written() {
        let engine = engine();
        let inbound = inbound();
        let naming = configured();
        let resolver = TermResolver::new(&engine, &inbound);
        let names = ForwardedNames::prepare(&engine, &resolver, &naming, Rewording::Reword)
            .await
            .expect("the forwarding context processes");
        let reworded = names.query_string("csf=speed%5Broad%5D%3D%3D%22Main%22").await.expect("rewords");
        let pairs = form_urlencoded::parse(reworded.as_bytes()).into_owned().collect::<Vec<_>>();
        pretty_assertions::assert_eq!(pairs, vec![("csf".to_owned(), "velocity[road]==\"Main\"".to_owned())]);
    }
}
