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

use crate::{
    error::FederationError,
    federation_invariant::FederationInvariant,
    forwarded_parameter::{ForwardedParameter, ParameterNames},
    forwarding_context::Rewording,
    term_resolver::TermResolver,
};
use andromeda_jsonld::{compaction::NameCompactor, context_spec::ContextSpec, engine::processor::JsonLdEngine, term_expansion::ContextualExpander};
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    entity_type_name::EntityTypeName,
    reference::{NgsiLdReference, NgsiLdReferenceError},
};
use andromeda_ngsi_ld_query::{
    filter::{compaction::compact_q_expr, expansion::expand_q_expr, expression::Expr, json_key_attributes::JsonKeyAttributes, parser},
    projection::grammar::{RawProjectionTerm, parse_projection_list},
    type_selection::grammar::TypeExprRaw,
};
use serde_json::{Map, Value};
use std::{future::Future, pin::Pin, str::FromStr};
use url::form_urlencoded;

/// The Subscription member holding its entity selectors (Table 5.2.12-1).
const ENTITIES_MEMBER: &str = "entities";
/// The entity selector member holding its type selection (Table 5.2.8-1).
const TYPE_MEMBER: &str = "type";
/// The Subscription member listing its watched attributes (Table 5.2.12-1).
const WATCHED_ATTRIBUTES_MEMBER: &str = "watchedAttributes";
/// The Subscription member holding its notification parameters (Table
/// 5.2.12-1).
const NOTIFICATION_MEMBER: &str = "notification";
/// The notification parameters member listing the notified attributes (Table
/// 5.2.14-1).
const NOTIFIED_ATTRIBUTES_MEMBER: &str = "attributes";
/// The Subscription member holding its clause 4.9 query (Table 5.2.12-1).
const Q_MEMBER: &str = "q";
/// The Subscription member holding its geoquery (Table 5.2.12-1).
const GEO_QUERY_MEMBER: &str = "geoQ";
/// The geoquery member naming the `GeoProperty` it evaluates (Table 5.2.13-1).
const GEOPROPERTY_MEMBER: &str = "geoproperty";

/// The names one forwarded request carries, spelled in its forwarding context.
pub(crate) struct ForwardedNames<'a> {
    /// Expands the client's names against the inbound context.
    resolver: &'a TermResolver<'a>,
    /// The engine the inbound expander reads.
    engine: &'a JsonLdEngine,
    /// Compacts names against the forwarding context.
    names: NameCompactor<'a>,
    /// Whether the forwarding context differs from the inbound one.
    rewording: Rewording,
}

impl<'a> ForwardedNames<'a> {
    /// The names a request forwarded under `naming` carries, reworded from the
    /// inbound context when `rewording` says the two differ.
    pub(crate) fn new(engine: &'a JsonLdEngine, resolver: &'a TermResolver<'a>, naming: &'a ContextSpec, rewording: Rewording) -> ForwardedNames<'a> {
        ForwardedNames {
            resolver,
            engine,
            names: NameCompactor::new(engine, naming),
            rewording,
        }
    }

    /// The inbound-context expansion this spelling is built on.
    pub(crate) const fn resolver(&self) -> &'a TermResolver<'a> {
        self.resolver
    }

    /// A registered attribute IRI spelled in the forwarding context, or the IRI
    /// itself when the context abbreviates none (clause 4.5.1).
    ///
    /// # Errors
    ///
    /// Returns [`FederationInvariant::ForwardedName`] when the compacted
    /// spelling is not an NGSI-LD name.
    pub(crate) async fn compact_attr(&self, iri: &AttributeName) -> Result<AttributeName, FederationError> {
        forwarded_name(&self.names.compact_attr_iri(iri.as_str()).await)
    }

    /// A registered entity type IRI spelled in the forwarding context, or the
    /// IRI itself when the context abbreviates none (clause 4.5.1).
    ///
    /// # Errors
    ///
    /// Returns [`FederationInvariant::ForwardedName`] when the compacted
    /// spelling is not an NGSI-LD name.
    pub(crate) async fn compact_type(&self, iri: &EntityTypeName) -> Result<EntityTypeName, FederationError> {
        forwarded_name(&self.names.compact_type_iri(iri.as_str()).await)
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
            Rewording::AsInbound => Ok(term.clone()), // clone: the forwarded query owns the names it writes
            Rewording::Reword => {
                let iri = self.resolver.expand_attr(term.as_str()).await?;
                forwarded_name(&self.names.compact_attr_iri(iri.as_str()).await)
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
            Rewording::AsInbound => Ok(term.clone()), // clone: the forwarded query owns the names it writes
            Rewording::Reword => {
                let iri = self.resolver.expand_type(term.as_str()).await?;
                forwarded_name(&self.names.compact_type_iri(iri.as_str()).await)
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
            Rewording::AsInbound => Ok(expression.clone()), // clone: the forwarded query owns the selector it writes
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
            Rewording::AsInbound => Ok(expression.clone()), // clone: the forwarded query owns the filter it writes
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
            Rewording::AsInbound => Ok(name.clone()), // clone: the forwarded query owns the name it writes
            Rewording::Reword => {
                let iri = self.resolver.expand_attr(name.as_str()).await?;
                forwarded_name(&self.names.compact_attr_iri(iri.as_str()).await)
            }
        }
    }

    /// A raw query string a client wrote, with every parameter that carries
    /// names (`type`, `attrs`, `jsonKeys`, `expandValues`, `geoproperty`,
    /// `pick`, `omit` and `q`) spelled in the forwarding context and every
    /// other parameter unchanged.
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::Query`] when a name-bearing parameter does
    /// not parse, and the errors of [`ForwardedNames::attr`] and
    /// [`ForwardedNames::q`] when one of its names does not reword.
    pub(crate) async fn query_string(&self, raw: &str) -> Result<String, FederationError> {
        if self.rewording == Rewording::AsInbound {
            return Ok(raw.to_owned());
        }
        let pairs = form_urlencoded::parse(raw.as_bytes())
            .into_owned()
            .map(|(key, value)| (key.parse::<ForwardedParameter>().ok(), key, value))
            .collect::<Vec<_>>();
        let mut json_keys = Vec::new();
        for (_, _, value) in pairs.iter().filter(|(parameter, _, _)| *parameter == Some(ForwardedParameter::JsonKeys)) {
            json_keys.extend(attribute_names(value)?);
        }
        // The serializer holds a non-`Sync` encoder, so it is built only once
        // every value is reworded and never lives across an await. A parameter
        // the binding tables do not name carries no name and travels as written.
        let mut reworded = Vec::with_capacity(pairs.len());
        for (parameter, key, value) in &pairs {
            let spelled = match parameter {
                Some(parameter) => self.reworded_parameter(*parameter, value, &json_keys).await?,
                None => value.clone(), // clone: the forwarded query owns the values it writes
            };
            reworded.push((key, spelled));
        }
        Ok(form_urlencoded::Serializer::new(String::new()).extend_pairs(reworded).finish())
    }

    /// One query parameter's value spelled in the forwarding context.
    async fn reworded_parameter(&self, parameter: ForwardedParameter, value: &str, json_keys: &[AttributeName]) -> Result<String, FederationError> {
        match parameter.names() {
            ParameterNames::TypeSelection => {
                let expression: TypeExprRaw = value.parse()?;
                Ok(self.type_expr(&expression).await?.to_string())
            }
            ParameterNames::AttributeList => {
                let mut names = Vec::new();
                for name in attribute_names(value)? {
                    names.push(self.attr(&name).await?.to_string());
                }
                Ok(names.join(","))
            }
            ParameterNames::Attribute => {
                let parsed: NgsiLdReference = value
                    .parse()
                    .map_err(|error| FederationError::from(FederationInvariant::ForwardedName(error)))?;
                Ok(self.geoproperty(&parsed).await?.to_string())
            }
            ParameterNames::Projection => {
                let terms = parse_projection_list(value)?;
                Ok(self.projection(&terms).await?.iter().map(ToString::to_string).collect::<Vec<_>>().join(","))
            }
            ParameterNames::Query => {
                let expression = parser::parse(value)?;
                Ok(self.q(&expression, Some(json_keys)).await?.to_string())
            }
            ParameterNames::SourceFilter => {
                let expression = parser::parse(value)?;
                Ok(self.q(&expression, None).await?.to_string())
            }
            ParameterNames::Nothing => Ok(value.to_owned()),
        }
    }

    /// A reduced subscription document (clause 5.8.1.4) with every name it
    /// carries spelled in the forwarding context: the entity selectors' types,
    /// `watchedAttributes`, `notification.attributes`, `q` and the geoquery's
    /// `geoproperty`. Every other member, which holds no name, is unchanged; the
    /// reduced copy carries neither `jsonKeys` nor `expandValues`.
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::Query`] when a member that carries names does
    /// not parse, and the errors of [`ForwardedNames::attr`] and
    /// [`ForwardedNames::q`] when one of its names does not reword.
    pub(crate) async fn subscription_document(&self, document: Value) -> Result<Value, FederationError> {
        let Value::Object(mut members) = document else {
            return Ok(document);
        };
        if self.rewording == Rewording::AsInbound {
            return Ok(Value::Object(members));
        }
        if let Some(Value::Array(selectors)) = members.get_mut(ENTITIES_MEMBER) {
            for selector in selectors.iter_mut().filter_map(Value::as_object_mut) {
                if let Some(Value::String(raw)) = selector.get_mut(TYPE_MEMBER) {
                    let expression: TypeExprRaw = raw.parse()?;
                    *raw = self.type_expr(&expression).await?.to_string();
                }
            }
        }
        self.reword_attribute_list(&mut members, WATCHED_ATTRIBUTES_MEMBER).await?;
        if let Some(Value::Object(notification)) = members.get_mut(NOTIFICATION_MEMBER) {
            self.reword_attribute_list(notification, NOTIFIED_ATTRIBUTES_MEMBER).await?;
        }
        if let Some(Value::String(raw)) = members.get_mut(Q_MEMBER) {
            let expression = parser::parse(raw)?;
            *raw = self.q(&expression, None).await?.to_string();
        }
        if let Some(Value::Object(geo)) = members.get_mut(GEO_QUERY_MEMBER)
            && let Some(Value::String(raw)) = geo.get_mut(GEOPROPERTY_MEMBER)
        {
            let parsed: NgsiLdReference = raw.parse().map_err(|error| FederationError::from(FederationInvariant::ForwardedName(error)))?;
            *raw = self.geoproperty(&parsed).await?.to_string();
        }
        Ok(Value::Object(members))
    }

    /// Reword the attribute names an array member of `object` holds.
    async fn reword_attribute_list(&self, object: &mut Map<String, Value>, member: &str) -> Result<(), FederationError> {
        let Some(Value::Array(names)) = object.get_mut(member) else {
            return Ok(());
        };
        for name in names.iter_mut() {
            if let Value::String(raw) = name {
                let parsed: AttributeName = raw.parse().map_err(|error| FederationError::from(FederationInvariant::ForwardedName(error)))?;
                *raw = self.attr(&parsed).await?.to_string();
            }
        }
        Ok(())
    }
}

/// The attribute names a comma-separated parameter value lists.
fn attribute_names(value: &str) -> Result<Vec<AttributeName>, FederationError> {
    value
        .split(',')
        .filter(|name| !name.is_empty())
        .map(|name| name.parse().map_err(|error| FederationError::from(FederationInvariant::ForwardedName(error))))
        .collect()
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
        ContextSpec::from_body(json!({
            "speed": "https://example.org/speed",
            "Vehicle": "https://example.org/Vehicle",
            "brand": "https://example.org/brand"
        }))
    }

    /// The forwarding context: the same IRIs under other terms.
    fn configured() -> ContextSpec {
        ContextSpec::from_body(json!({
            "velocity": "https://example.org/speed",
            "Car": "https://example.org/Vehicle"
        }))
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
        let names = ForwardedNames::new(&engine, &resolver, &naming, Rewording::Reword);

        pretty_assertions::assert_eq!(names.attr(&attr("speed")).await.expect("rewords").as_str(), "velocity");
        // A term the forwarding context does not abbreviate travels as its IRI.
        pretty_assertions::assert_eq!(names.attr(&attr("brand")).await.expect("rewords").as_str(), "https://example.org/brand");
        let vehicle: EntityTypeName = "Vehicle".parse().expect("a type name");
        pretty_assertions::assert_eq!(names.entity_type(&vehicle).await.expect("rewords").as_str(), "Car");
        let expression: TypeExprRaw = "Vehicle,Other".parse().expect("a type selection");
        pretty_assertions::assert_eq!(names.type_expr(&expression).await.expect("rewords").to_string(), "Car,Other");
        // An IRI carries dots, so the clause 4.9 serialisation quotes it.
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
        let names = ForwardedNames::new(&engine, &resolver, &inbound, Rewording::AsInbound);
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
        let names = ForwardedNames::new(&engine, &resolver, &naming, Rewording::Reword);
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
        let inbound = ContextSpec::from_body(json!({
            "speed": "https://example.org/speed",
            "road": "https://example.org/street"
        }));
        let naming = ContextSpec::from_body(json!({
            "velocity": "https://example.org/speed",
            "street": "https://example.org/street"
        }));
        let resolver = TermResolver::new(&engine, &inbound);
        let names = ForwardedNames::new(&engine, &resolver, &naming, Rewording::Reword);

        let literal = names.query_string("q=speed%5Broad%5D%3D%3D%22Main%22&jsonKeys=speed").await.expect("rewords");
        let pairs = form_urlencoded::parse(literal.as_bytes()).into_owned().collect::<Vec<_>>();
        let value = |key: &str| pairs.iter().find(|(name, _)| name == key).map(|(_, value)| value.clone()); // clone: the assertion owns the value
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
        let names = ForwardedNames::new(&engine, &resolver, &naming, Rewording::Reword);
        let reworded = names.query_string("csf=speed%3D%3D1&orderBy=speed").await.expect("rewords");
        let pairs = form_urlencoded::parse(reworded.as_bytes()).into_owned().collect::<Vec<_>>();
        pretty_assertions::assert_eq!(
            pairs,
            vec![("csf".to_owned(), "velocity==1".to_owned()), ("orderBy".to_owned(), "speed".to_owned())]
        );
    }

    #[tokio::test]
    async fn a_subscription_document_rewords_every_name_it_carries() {
        let engine = engine();
        let inbound = inbound();
        let naming = configured();
        let resolver = TermResolver::new(&engine, &inbound);
        let names = ForwardedNames::new(&engine, &resolver, &naming, Rewording::Reword);
        let document = json!({
            "id": "urn:ngsi-ld:Subscription:forwarded-1",
            "type": "Subscription",
            "entities": [{"type": "Vehicle"}],
            "watchedAttributes": ["speed"],
            "q": "speed>10",
            "geoQ": {"geometry": "Point", "coordinates": [1, 2], "georel": "near;maxDistance==10", "geoproperty": "speed"},
            "notification": {"attributes": ["speed", "brand"], "endpoint": {"uri": "http://receiver.example.org/notify"}}
        });
        let reworded = names.subscription_document(document).await.expect("rewords");
        pretty_assertions::assert_eq!(reworded["entities"][0]["type"], json!("Car"));
        pretty_assertions::assert_eq!(reworded["watchedAttributes"], json!(["velocity"]));
        pretty_assertions::assert_eq!(reworded["q"], json!("velocity>10"));
        pretty_assertions::assert_eq!(reworded["geoQ"]["geoproperty"], json!("velocity"));
        pretty_assertions::assert_eq!(reworded["notification"]["attributes"], json!(["velocity", "https://example.org/brand"]));
        pretty_assertions::assert_eq!(reworded["notification"]["endpoint"]["uri"], json!("http://receiver.example.org/notify"));
        pretty_assertions::assert_eq!(reworded["type"], json!("Subscription"));
    }
}
