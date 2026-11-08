//! The `Location` a creating operation answers with.
//!
//! Each creating operation answers with a `Location` holding "the relative
//! path of the created" resource (Tables 6.4.3.1-1, 6.8.3.1-1, 6.10.3.1-1,
//! 6.12.3.1-1, 6.18.3.1-1, 6.29.3.1-1, 6.36.3.1-1 and 6.38.3.1-1). The resource
//! sits at `{apiRoot}/ngsi-ld/v1/...` (clause 6.2), and an `apiRoot` may carry
//! "an optional prefix string" that a reverse proxy strips before the request
//! reaches the broker. The path therefore starts with the configured
//! `apiRoot`'s own path, so a client resolving it against the URI it addressed
//! reaches the resource through the same prefix. The scheme and authority stay
//! out: the tables prescribe a path.

use crate::api_root::{ApiRoot, ConfiguredApiRoot};
use andromeda_http_headers::location::Location;
use andromeda_http_response::headers::with_location;
use axum::{extract::FromRequestParts, http::request::Parts, response::Response};
use std::{
    convert::Infallible,
    fmt::Display,
    future::{Future, ready},
};
use url::Url;

/// A collection a creating operation adds a resource to, as its path below
/// `/ngsi-ld/v1/` (Figure 6.2-1).
#[derive(Clone, Copy, Debug, Eq, PartialEq, strum::IntoStaticStr)]
pub enum ResourceCollection {
    /// Entities (clause 6.4).
    #[strum(serialize = "entities")]
    Entities,
    /// Temporal Evolutions of Entities (clause 6.18).
    #[strum(serialize = "temporal/entities")]
    TemporalEntities,
    /// `EntityMap`s created by an entity query (clause 6.34).
    #[strum(serialize = "entityMaps")]
    EntityMaps,
    /// `EntityMap`s created by a temporal query (clause 6.35).
    #[strum(serialize = "temporal/entityMaps")]
    TemporalEntityMaps,
    /// Context Source Registrations (clause 6.8).
    #[strum(serialize = "csourceRegistrations")]
    ContextSourceRegistrations,
    /// Subscriptions (clause 6.10).
    #[strum(serialize = "subscriptions")]
    Subscriptions,
    /// Context Source Registration Subscriptions (clause 6.12).
    #[strum(serialize = "csourceSubscriptions")]
    ContextSourceSubscriptions,
    /// Snapshots (clause 6.36).
    #[strum(serialize = "snapshots")]
    Snapshots,
    /// Stored `@context`s (clause 6.29).
    #[strum(serialize = "jsonldContexts")]
    JsonldContexts,
}

/// Names created resources below the `apiRoot`'s path, or below the root
/// when there is none.
///
/// Extracted from the [`ConfiguredApiRoot`] request extension alone, so
/// extracting it never fails. The origin a request reveals has the path `/`,
/// so a locator built from an [`ApiRoot`] names the same paths.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResourceLocator(Option<Url>);

impl ResourceLocator {
    /// A locator naming resources below `api_root`'s path, or below the root
    /// when `None`.
    #[must_use]
    pub const fn new(api_root: Option<Url>) -> ResourceLocator {
        ResourceLocator(api_root)
    }

    /// `{prefix}/ngsi-ld/v1/{collection}/{identifier}`, or `None` when the
    /// identifier cannot travel in a header field.
    #[must_use]
    pub fn location(&self, collection: ResourceCollection, identifier: &impl Display) -> Option<Location> {
        let prefix = self.0.as_ref().map_or("", |root| root.path().trim_end_matches('/'));
        let collection: &'static str = collection.into();
        Location::new(format!("{prefix}/ngsi-ld/v1/{collection}/{identifier}"))
    }

    /// `response` with the `Location` of the resource it created.
    #[must_use]
    pub fn locate(&self, response: Response, collection: ResourceCollection, identifier: &impl Display) -> Response {
        match self.location(collection, identifier) {
            Some(location) => with_location(response, location),
            None => response,
        }
    }
}

impl<S> FromRequestParts<S> for ResourceLocator
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    fn from_request_parts(parts: &mut Parts, _state: &S) -> impl Future<Output = Result<Self, Self::Rejection>> {
        // clone: the configured URL stays in the request's extensions for later extractors
        ready(Ok(ResourceLocator(parts.extensions.get::<ConfiguredApiRoot>().map(|root| root.url().clone()))))
    }
}

impl From<&ApiRoot> for ResourceLocator {
    fn from(api_root: &ApiRoot) -> ResourceLocator {
        ResourceLocator(api_root.url().cloned()) // clone: the request keeps its apiRoot for the URLs it still mints
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        api_root::{ApiRoot, ConfiguredApiRoot},
        resource_location::{ResourceCollection, ResourceLocator},
    };
    use axum::{
        extract::FromRequestParts,
        http::{Request, StatusCode},
        response::IntoResponse,
    };
    use std::error::Error;
    use url::Url;

    const ID: &str = "urn:ngsi-ld:Vehicle:1";

    fn locator(root: &str) -> Result<ResourceLocator, url::ParseError> {
        Ok(ResourceLocator::new(Some(Url::parse(root)?)))
    }

    /// The `Location` text `locator` gives a created resource of `collection`.
    fn location_text(locator: &ResourceLocator, collection: ResourceCollection) -> Option<String> {
        let response = locator.locate(StatusCode::CREATED.into_response(), collection, &ID);
        response.headers().get("location").and_then(|value| value.to_str().ok()).map(str::to_owned)
    }

    #[test]
    fn without_a_configured_api_root_the_path_starts_at_the_root() {
        pretty_assertions::assert_eq!(
            location_text(&ResourceLocator::default(), ResourceCollection::Entities),
            Some(format!("/ngsi-ld/v1/entities/{ID}"))
        );
    }

    /// Clause 6.2: the prefix string of the configured `apiRoot` leads the
    /// path, with or without its trailing slash.
    #[test]
    fn a_configured_api_root_prefix_leads_the_path() -> Result<(), url::ParseError> {
        for root in ["https://gw.example.com/broker", "https://gw.example.com/broker/"] {
            pretty_assertions::assert_eq!(
                location_text(&locator(root)?, ResourceCollection::Subscriptions),
                Some(format!("/broker/ngsi-ld/v1/subscriptions/{ID}"))
            );
        }
        Ok(())
    }

    /// The tables prescribe a relative path, so the configured scheme and
    /// authority stay out of it.
    #[test]
    fn a_configured_api_root_without_a_prefix_gives_the_bare_path() -> Result<(), url::ParseError> {
        pretty_assertions::assert_eq!(
            location_text(&locator("http://broker:1071")?, ResourceCollection::Snapshots),
            Some(format!("/ngsi-ld/v1/snapshots/{ID}"))
        );
        Ok(())
    }

    #[test]
    fn each_collection_names_its_own_path() -> Result<(), url::ParseError> {
        let locator = locator("https://gw.example.com/a/b")?;
        for (collection, path) in [
            (ResourceCollection::Entities, "entities"),
            (ResourceCollection::TemporalEntities, "temporal/entities"),
            (ResourceCollection::EntityMaps, "entityMaps"),
            (ResourceCollection::TemporalEntityMaps, "temporal/entityMaps"),
            (ResourceCollection::ContextSourceRegistrations, "csourceRegistrations"),
            (ResourceCollection::Subscriptions, "subscriptions"),
            (ResourceCollection::ContextSourceSubscriptions, "csourceSubscriptions"),
            (ResourceCollection::Snapshots, "snapshots"),
            (ResourceCollection::JsonldContexts, "jsonldContexts"),
        ] {
            pretty_assertions::assert_eq!(location_text(&locator, collection), Some(format!("/a/b/ngsi-ld/v1/{path}/{ID}")));
        }
        Ok(())
    }

    #[tokio::test]
    async fn the_locator_reads_the_installed_api_root() -> Result<(), Box<dyn Error>> {
        let (mut parts, ()) = Request::post("/ngsi-ld/v1/entities").header("host", "internal.example").body(())?.into_parts();
        parts.extensions.insert(ConfiguredApiRoot::new(Url::parse("https://gw.example.com/broker")?));
        let located = ResourceLocator::from_request_parts(&mut parts, &()).await?;
        pretty_assertions::assert_eq!(
            location_text(&located, ResourceCollection::Entities),
            Some(format!("/broker/ngsi-ld/v1/entities/{ID}"))
        );
        Ok(())
    }

    /// The origin the request reveals carries no prefix, so it is not read.
    #[tokio::test]
    async fn without_an_installed_api_root_the_request_origin_is_not_read() -> Result<(), Box<dyn Error>> {
        let (mut parts, ()) = Request::post("http://internal.example/x/ngsi-ld/v1/entities").body(())?.into_parts();
        let located = ResourceLocator::from_request_parts(&mut parts, &()).await?;
        pretty_assertions::assert_eq!(located, ResourceLocator::default());
        Ok(())
    }

    /// A request origin has the path `/`, so it gives the same path as no
    /// `apiRoot` at all.
    #[test]
    fn a_request_origin_api_root_gives_the_bare_path() -> Result<(), url::ParseError> {
        let api_root = ApiRoot::new(Some(Url::parse("http://internal.example:1026")?));
        pretty_assertions::assert_eq!(
            location_text(&ResourceLocator::from(&api_root), ResourceCollection::EntityMaps),
            Some(format!("/ngsi-ld/v1/entityMaps/{ID}"))
        );
        Ok(())
    }

    #[test]
    fn a_configured_api_root_resolved_for_the_request_keeps_its_prefix() -> Result<(), url::ParseError> {
        let api_root = ApiRoot::new(Some(Url::parse("https://gw.example.com/broker")?));
        pretty_assertions::assert_eq!(
            location_text(&ResourceLocator::from(&api_root), ResourceCollection::TemporalEntityMaps),
            Some(format!("/broker/ngsi-ld/v1/temporal/entityMaps/{ID}"))
        );
        Ok(())
    }
}
