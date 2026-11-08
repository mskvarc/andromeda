//! Typed, state-complete source-identity route inventory.

use crate::{handler, state::SourceIdentityState};
use axum::{Router, routing::get};

/// The complete source-identity route fragment.
pub struct SourceIdentityRoutes {
    /// Router whose focused state has already been supplied.
    router: Router,
}

impl SourceIdentityRoutes {
    /// Construct `GET /info/sourceIdentity` with focused immutable state.
    pub fn new(state: SourceIdentityState) -> SourceIdentityRoutes {
        let router = Router::new().route("/sourceIdentity", get(handler::get)).with_state(state);
        SourceIdentityRoutes {
            router: Router::new().nest("/info", router),
        }
    }

    /// Consume the state-complete route fragment.
    pub fn into_router(self) -> Router {
        self.router
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        default_tenant_contexts::{DefaultTenantContexts, DefaultTenantContextsResolver},
        routes::SourceIdentityRoutes,
        state::SourceIdentityState,
    };
    use andromeda_context_registry::{
        context_page::ContextPage,
        creation_kind::ContextCreationKind,
        detail_level::ContextDetailLevel,
        fetched_context::FetchedContext,
        insertion::ContextInsertion,
        list_query::ContextListQuery,
        listed_contexts::ListedContexts,
        mutation::ContextMutation,
        service::ContextRegistryService,
        store::{ContextRegistryFuture, ContextRegistryStore},
        stored_context::StoredContext,
    };
    use andromeda_extensions::enabled::EnabledExtensions;
    use andromeda_http_context_registry::tenant_context_hosting::TenantContextHosting;
    use andromeda_json_literal::JsonLiteral;
    use andromeda_jsonld::{context_cache::DownloadedContext, engine::processor::JsonLdEngine};
    use andromeda_ngsi_ld_lexicon::{context_source_pseudonym::ContextSourcePseudonym, identifier::Identifier, ngsi_ld_urn::JsonLdContextId};
    use andromeda_ngsi_ld_query::page_reach::PageReach;
    use andromeda_tenancy::{
        context_source_alias::ContextSourceAliases,
        creation_policy::TenantCreationPolicy,
        error::TenancyError,
        resident_tenancy::ResidentTenancy,
        runtime::lease::LeasedState,
        tenant_ref::TenantRef,
    };
    use axum::{
        body::{Body, to_bytes},
        http::{Request, Response, StatusCode},
    };
    use chrono::Utc;
    use futures::future::BoxFuture;
    use serde_json::Value;
    use std::{
        collections::HashMap,
        error::Error,
        fs,
        sync::{Arc, Mutex},
    };
    use tempfile::TempDir;
    use tower::ServiceExt;
    use url::Url;

    /// The user context the Link test names, preloaded so no request leaves
    /// the process.
    const USER_CONTEXT: &str = "https://contexts.example/user.jsonld";

    /// A store holding nothing, recording the identifier of every insert.
    #[derive(Default)]
    struct RecordingStore {
        /// Every identifier inserted, in order.
        inserted: Mutex<Vec<JsonLdContextId>>,
    }

    impl RecordingStore {
        /// Every identifier inserted so far.
        fn inserted(&self) -> Vec<JsonLdContextId> {
            self.inserted.lock().map(|inserted| inserted.clone()).unwrap_or_default()
        }
    }

    impl ContextRegistryStore for RecordingStore {
        fn insert<'a>(
            &'a self,
            identifier: &'a JsonLdContextId,
            _payload: &'a JsonLiteral,
            _serving_url: &'a Url,
            _kind: ContextCreationKind,
        ) -> ContextRegistryFuture<'a, ContextInsertion> {
            if let Ok(mut inserted) = self.inserted.lock() {
                inserted.push(identifier.clone());
            }
            Box::pin(async { Ok(ContextInsertion::Inserted) })
        }

        fn list<'a>(&'a self, _query: ContextListQuery<'a>) -> ContextRegistryFuture<'a, ContextPage> {
            Box::pin(async {
                Ok(ContextPage {
                    contexts: ListedContexts::Summary(Vec::new()),
                    boundary: None,
                    reach: PageReach::Exhausted,
                })
            })
        }

        fn get<'a>(&'a self, _identifier: &'a Identifier, _detail_level: ContextDetailLevel) -> ContextRegistryFuture<'a, Option<FetchedContext>> {
            Box::pin(async { Ok(None) })
        }

        fn record_access<'a>(&'a self, _identifier: &'a Identifier) -> ContextRegistryFuture<'a, ContextMutation> {
            Box::pin(async { Ok(ContextMutation::Missing) })
        }

        fn lookup<'a>(&'a self, _identifier: &'a Identifier) -> ContextRegistryFuture<'a, Option<StoredContext>> {
            Box::pin(async { Ok(None) })
        }

        fn delete<'a>(&'a self, _identifier: &'a Identifier) -> ContextRegistryFuture<'a, ContextMutation> {
            Box::pin(async { Ok(ContextMutation::Missing) })
        }

        fn replace_download<'a>(&'a self, _identifier: &'a Identifier, _download: &'a DownloadedContext) -> ContextRegistryFuture<'a, ContextMutation> {
            Box::pin(async { Ok(ContextMutation::Missing) })
        }
    }

    /// A default tenant with an offline engine, the user context preloaded
    /// and a recording store, resolved through a genuine runtime cache.
    struct OfflineDefaultTenant {
        /// The engine every response resolves its context with.
        engine: JsonLdEngine,
        /// The cache the default tenant's lease comes from.
        tenancy: ResidentTenancy<TenantContextHosting>,
        /// Holds the preloaded user context for the engine's lifetime.
        _preloaded: TempDir,
    }

    impl OfflineDefaultTenant {
        fn new(store: Arc<RecordingStore>) -> Result<OfflineDefaultTenant, Box<dyn Error + Send + Sync>> {
            let preloaded = TempDir::new()?;
            let user_context = preloaded.path().join("user.jsonld");
            fs::write(&user_context, r#"{ "@context": { "Spot": "https://contexts.example/Spot" } }"#)?;
            let engine = JsonLdEngine::builder()
                .allow_remote(false)
                .preload_contexts(HashMap::from([(Url::parse(USER_CONTEXT)?, user_context)]))
                .build()?;
            let contexts = Arc::new(ContextRegistryService::new(store, engine.clone()));
            let hosting = Arc::new(TenantContextHosting::new(contexts));
            Ok(OfflineDefaultTenant {
                engine,
                tenancy: ResidentTenancy::new(hosting, Vec::new()),
                _preloaded: preloaded,
            })
        }
    }

    impl DefaultTenantContextsResolver for OfflineDefaultTenant {
        fn resolve(&self) -> BoxFuture<'_, Result<DefaultTenantContexts, TenancyError>> {
            Box::pin(async {
                let LeasedState { state, lease } = self.tenancy.resolve(&TenantRef::UnnamedDefault, TenantCreationPolicy::MustExist).await?;
                Ok(DefaultTenantContexts {
                    engine: self.engine.clone(),
                    context_hosting: TenantContextHosting::clone(&state),
                    runtime_lease: lease,
                })
            })
        }
    }

    /// The source-identity routes over `store`.
    fn routes(store: Arc<RecordingStore>) -> Result<SourceIdentityRoutes, Box<dyn Error + Send + Sync>> {
        let state = SourceIdentityState::new(
            "urn:ngsi-ld:ContextSourceIdentity:test".parse::<Identifier>()?,
            ContextSourceAliases::new("test".parse::<ContextSourcePseudonym>()?),
            Utc::now(),
            None,
            EnabledExtensions::none(),
            Arc::new(OfflineDefaultTenant::new(store)?),
        )?;
        Ok(SourceIdentityRoutes::new(state))
    }

    /// The answer to `request` over an empty recording store.
    async fn answer(request: Request<Body>) -> Result<Response<Body>, Box<dyn Error + Send + Sync>> {
        Ok(routes(Arc::new(RecordingStore::default()))?.into_router().oneshot(request).await?)
    }

    /// The identity document the fragment answers `tenant` with.
    async fn identity_for(tenant: Option<&str>) -> Result<(StatusCode, Value), Box<dyn Error + Send + Sync>> {
        let mut request = Request::get("/info/sourceIdentity").header("host", "broker.example");
        if let Some(tenant) = tenant {
            request = request.header("NGSILD-Tenant", tenant);
        }
        let response = answer(request.body(Body::empty())?).await?;
        let status = response.status();
        let body = to_bytes(response.into_body(), usize::MAX).await?;
        Ok((status, serde_json::from_slice(&body)?))
    }

    #[tokio::test]
    async fn source_identity_fragment_owns_its_complete_path() -> Result<(), Box<dyn Error + Send + Sync>> {
        let (status, _) = identity_for(None).await?;
        pretty_assertions::assert_eq!(status, StatusCode::OK);
        Ok(())
    }

    /// Table 5.2.40-1: the alias identifies the Tenant the request addressed,
    /// so the route reads the clause 6.3.14 header rather than ignoring it.
    #[tokio::test]
    async fn the_route_answers_each_tenant_with_its_own_alias() -> Result<(), Box<dyn Error + Send + Sync>> {
        let (_, default) = identity_for(None).await?;
        let (_, alpha) = identity_for(Some("alpha")).await?;
        pretty_assertions::assert_eq!(default["contextSourceAlias"], Value::from("test"));
        pretty_assertions::assert_eq!(alpha["contextSourceAlias"], Value::from("test~alpha"));
        pretty_assertions::assert_eq!(default["id"], alpha["id"]);
        Ok(())
    }

    /// Clause 6.3.14 makes an empty `NGSILD-Tenant` no tenant at all, and the
    /// shared extractor answers it with the same rejection every route uses.
    #[tokio::test]
    async fn an_empty_tenant_header_is_rejected_rather_than_read_as_the_default() -> Result<(), Box<dyn Error + Send + Sync>> {
        let (status, _) = identity_for(Some("")).await?;
        pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
        Ok(())
    }

    /// Clause 6.3.20: the operation defines no parameter, so any parameter is
    /// an `InvalidRequest` here.
    #[tokio::test]
    async fn an_undefined_parameter_is_invalid_request() -> Result<(), Box<dyn Error + Send + Sync>> {
        let response = answer(
            Request::get("/info/sourceIdentity?bogus=x")
                .header("host", "broker.example")
                .body(Body::empty())?,
        )
        .await?;
        pretty_assertions::assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX).await?;
        let problem: Value = serde_json::from_slice(&body)?;
        assert!(problem["type"].as_str().is_some_and(|uri| uri.ends_with("/InvalidRequest")), "{problem}");
        Ok(())
    }

    /// Clause 6.3.6 and Annex C.8: an `application/json` response to a request
    /// naming a user context names it with the core context through one hosted
    /// wrapper, stored among the default tenant's `@context`s (clause 4.14)
    /// and named by the response `Link`.
    #[tokio::test]
    async fn a_user_context_hosts_one_wrapper_in_the_default_tenant_and_the_link_names_it() -> Result<(), Box<dyn Error + Send + Sync>> {
        let store = Arc::new(RecordingStore::default());
        let request = Request::get("/info/sourceIdentity")
            .header("host", "broker.example")
            .header("accept", "application/json")
            .header(
                "link",
                format!("<{USER_CONTEXT}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\""),
            )
            .body(Body::empty())?;

        let response = routes(Arc::clone(&store))?.into_router().oneshot(request).await?;

        pretty_assertions::assert_eq!(response.status(), StatusCode::OK);
        let inserted = store.inserted();
        pretty_assertions::assert_eq!(inserted.len(), 1, "exactly one wrapper is hosted: {inserted:?}");
        let link = response.headers().get("link").and_then(|value| value.to_str().ok()).unwrap_or_default();
        let wrapper = inserted.first().map(ToString::to_string).unwrap_or_default();
        assert!(
            link.contains(&format!("http://broker.example/ngsi-ld/v1/jsonldContexts/{wrapper}>")),
            "the Link names the hosted wrapper, untenanted: {link}"
        );
        Ok(())
    }
}
