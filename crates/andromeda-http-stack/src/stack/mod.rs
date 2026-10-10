//! Deterministic application of API-wide and process-wide middleware.

mod compression_layer;
mod cors_layer;
pub mod csrf_layer;
mod decompression_layer;
mod encoded_entity_tag;
mod helmet_layer;
mod metric_layer;
mod server_timing;

use crate::{
    config::{HttpStackConfig, server_timing::ServerTimingConfig},
    stack::{
        compression_layer::build_compression_layer,
        cors_layer::build_cors_layer,
        csrf_layer::{CsrfLayerError, build_csrf_layer},
        decompression_layer::{build_decompression_layer, record_received_content_length},
        encoded_entity_tag::EncodedEntityTags,
        helmet_layer::build_helmet_layer,
        metric_layer::http_metric_layer,
        server_timing::apply_server_timing,
    },
};
use andromeda_http_extraction::{blocking_decode::BlockingDecodePolicy, request_body_limit::RequestBodyLimit, request_json_value_limit::RequestJsonValueLimit};
use andromeda_http_headers::{
    forwarded_header_trust::ForwardedHeaderTrust,
    link::field_layout::LinkFieldLayout,
    ngsild_tenant::NgsiLdTenant,
    x_request_id::XRequestId,
};
use andromeda_http_tenancy::propagation::echo_tenant_header;
use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
use axum::{
    Extension,
    Router,
    extract::{DefaultBodyLimit, MatchedPath, Request},
    http::{HeaderMap, StatusCode, header},
    middleware,
    response::Response,
};
use axum_helmet::{HelmetError, HelmetLayer};
use headers::{Header, HeaderMapExt};
use std::{sync::Arc, time::Duration};
use thiserror::Error;
use tower::{Layer, ServiceBuilder};
use tower_http::{
    compression::CompressionLayer,
    cors::CorsLayer,
    csrf::CsrfLayer,
    decompression::RequestDecompressionLayer,
    normalize_path::{NormalizePath, NormalizePathLayer},
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, RequestId, SetRequestIdLayer},
    sensitive_headers::{SetSensitiveRequestHeadersLayer, SetSensitiveResponseHeadersLayer},
    timeout::TimeoutLayer,
    trace::TraceLayer,
};

/// A fallible middleware layer could not be built from its configuration.
#[derive(Debug, Error)]
pub enum HttpStackBuildError {
    /// The security-header layer could not be rendered.
    #[error("failed to build the security-header layer: {0}")]
    Helmet(#[from] HelmetError),
    /// A CSRF trusted origin could not be admitted.
    #[error(transparent)]
    Csrf(#[from] CsrfLayerError),
}

/// Explicitly ordered inbound HTTP stack.
///
/// The fallible layers (security headers, CSRF) are built once at construction
/// and stored, so no per-request rebuild happens and no build failure can occur
/// after the broker starts serving. The prebuilt optional layers are `None` when
/// the configuration switched the feature off.
#[derive(Clone)]
pub struct HttpStack {
    /// API-wide body admission limit.
    request_body_limit: RequestBodyLimit,
    /// API-wide limit on the JSON values one payload may hold.
    request_json_value_limit: RequestJsonValueLimit,
    /// How a payload is decoded beside the executor.
    blocking_decode: BlockingDecodePolicy,
    /// The operator's forwarded-header trust decision.
    forwarded_headers: ForwardedHeaderTrust,
    /// How a response's link-values are laid out over `Link` field lines.
    link_field: LinkFieldLayout,
    /// Maximum wall-clock time for one HTTP request.
    request_timeout: Duration,
    /// `Server-Timing` policy, applied per assembled router.
    server_timing: ServerTimingConfig,
    /// The prebuilt security-header layer, absent when disabled.
    helmet: Option<HelmetLayer>,
    /// The prebuilt CORS layer, absent when disabled.
    cors: Option<CorsLayer>,
    /// The prebuilt response-compression layer, absent when disabled.
    compression: Option<CompressionLayer>,
    /// The validator rewrite matching the codings the compression layer applies.
    encoded_entity_tags: EncodedEntityTags,
    /// The prebuilt request-decompression layer, absent when disabled.
    decompression: Option<RequestDecompressionLayer>,
    /// The prebuilt CSRF layer, absent when disabled.
    csrf: Option<CsrfLayer>,
}

impl HttpStack {
    /// Construct an HTTP stack from its owner-defined configuration.
    ///
    /// # Errors
    ///
    /// Returns [`HttpStackBuildError`] when the security-header layer cannot be
    /// rendered or a CSRF trusted origin is not one the CSRF layer accepts.
    pub fn new(config: HttpStackConfig) -> Result<HttpStack, HttpStackBuildError> {
        // A 64-bit host has `usize == u64`, so the conversion is exact; the
        // saturating fallback keeps a hypothetical 32-bit build from panicking on
        // an over-large configured ceiling rather than silently truncating it.
        let request_body_limit = RequestBodyLimit::new(usize::try_from(config.max_request_body.as_u64()).unwrap_or(usize::MAX));
        let blocking_decode = BlockingDecodePolicy::new(
            usize::try_from(config.blocking_decode_threshold.as_u64()).unwrap_or(usize::MAX),
            config.max_concurrent_blocking_decodes,
        );
        Ok(HttpStack {
            request_body_limit,
            request_json_value_limit: RequestJsonValueLimit::new(JsonValueLimit::new(config.max_request_json_values)),
            blocking_decode,
            forwarded_headers: config.forwarded_headers,
            link_field: config.link_field,
            request_timeout: config.request_timeout.duration(),
            helmet: build_helmet_layer(config.helmet)?,
            cors: build_cors_layer(&config.cors),
            compression: build_compression_layer(&config.compression),
            encoded_entity_tags: EncodedEntityTags::new(config.compression.algorithms),
            decompression: build_decompression_layer(&config.decompression),
            csrf: build_csrf_layer(&config.csrf)?,
            server_timing: config.server_timing,
        })
    }

    /// Apply middleware scoped to the complete NGSI-LD API subtree.
    pub fn apply_api(&self, router: Router) -> Router {
        // Every absolute URL the broker mints from a request origin reads the
        // operator's trust decision from this extension, so a router assembled
        // without the stack treats every forwarded-protocol field as untrusted
        // rather than inheriting a permissive default.
        let mut router = router
            .layer(Extension(self.forwarded_headers))
            // The limit travels with the request as well as bounding it, so a
            // payload reader refusing a body names the size it would accept.
            .layer(Extension(self.request_body_limit))
            .layer(DefaultBodyLimit::max(self.request_body_limit.bytes()))
            // Every payload parser counts the payload's values against this
            // limit before it builds a tree, and decodes a payload this large
            // or larger beside the executor.
            .layer(Extension(self.request_json_value_limit))
            .layer(Extension(self.blocking_decode.clone())); // clone: every request shares the stack's decode permits
        if let Some(helmet) = &self.helmet {
            router = router.layer(helmet.clone());
        }
        // Applied to the assembled response, so the `@context` link and the
        // pagination links each handler writes are laid out once, whichever
        // handler wrote them.
        let link_field = self.link_field;
        router.layer(middleware::map_response(move |mut response: Response| async move {
            link_field.apply(response.headers_mut());
            response
        }))
    }

    /// Apply the shared diagnostic layers common to every listener.
    ///
    /// Request-id assignment and propagation, sensitive-header redaction, the
    /// request/response trace, and the per-request timeout are diagnostics every
    /// plane needs regardless of its edge policy, so both the process stack and
    /// the bare management stack build on this one core rather than duplicating
    /// it. The request-metric layer is intentionally excluded: only the process
    /// stack instruments requests, and it adds the metric layer outside this
    /// core so the metric spans every request.
    fn apply_diagnostic_layers(&self, router: Router) -> Router {
        let sensitive_request_headers = Arc::<[_]>::from([header::AUTHORIZATION, header::COOKIE, header::PROXY_AUTHORIZATION]);
        let sensitive_response_headers = Arc::<[_]>::from([header::SET_COOKIE]);
        let trace_layer = TraceLayer::new_for_http()
            .make_span_with(|request: &Request| {
                let path = request
                    .extensions()
                    .get::<MatchedPath>()
                    .map_or_else(|| request.uri().path(), MatchedPath::as_str);
                let request_id = request
                    .extensions()
                    .get::<RequestId>()
                    .and_then(|id| id.header_value().to_str().ok())
                    .map(str::to_owned)
                    .or_else(|| request.headers().typed_get::<XRequestId>().map(|id| id.as_str().to_owned()))
                    .unwrap_or_default();
                let tenant = request
                    .headers()
                    .get(NgsiLdTenant::name())
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or_default();
                tracing::info_span!(
                    "http-request",
                    method = %request.method(),
                    path = %path,
                    request_id = %request_id,
                    tenant = %tenant,
                )
            })
            .on_request(|request: &Request, _span: &tracing::Span| {
                let request_id = request
                    .extensions()
                    .get::<RequestId>()
                    .and_then(|id| id.header_value().to_str().ok())
                    .map(str::to_owned)
                    .or_else(|| request.headers().typed_get::<XRequestId>().map(|id| id.as_str().to_owned()))
                    .unwrap_or_default();
                tracing::info!(request_id = %request_id, method = %request.method(), uri = %request.uri(), "started request");
            })
            .on_response(|response: &Response, latency: Duration, _span: &tracing::Span| {
                let request_id = response
                    .headers()
                    .typed_get::<XRequestId>()
                    .map_or_else(String::new, |id| id.as_str().to_owned());
                tracing::info!(request_id = %request_id, status = %response.status(), latency_us = latency.as_micros(), "finished request");
            });
        let diagnostic = ServiceBuilder::new()
            .layer(SetSensitiveRequestHeadersLayer::from_shared(sensitive_request_headers))
            .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
            .layer(trace_layer)
            .layer(SetSensitiveResponseHeadersLayer::from_shared(sensitive_response_headers))
            .layer(PropagateRequestIdLayer::x_request_id())
            .layer(TimeoutLayer::with_status_code(StatusCode::REQUEST_TIMEOUT, self.request_timeout));
        router.layer(diagnostic)
    }

    /// Apply process-wide middleware in deterministic order and normalize paths before routing.
    pub fn apply_process(&self, router: Router) -> NormalizePath<Router> {
        let mut router = router.layer(middleware::from_fn(echo_tenant_header));
        router = apply_server_timing(router, &self.server_timing);
        if let Some(compression) = &self.compression {
            // Wrapped around the compression layer, so it sees the response as
            // encoded and weakens a strong tag the handler computed over the
            // unencoded bytes (RFC 7232 section 2.3.3), and gives a 304 the
            // validators of the encoded 200 it stands for (section 4.1).
            let tags = self.encoded_entity_tags;
            router = router
                .layer(compression.clone())
                .layer(middleware::map_response(move |request_headers: HeaderMap, response: Response| async move {
                    tags.revalidated(&request_headers, response)
                }));
        }
        if let Some(decompression) = &self.decompression {
            router = router
                .layer(decompression.clone())
                .layer(middleware::map_request(record_received_content_length));
        }
        router = self.apply_diagnostic_layers(router);
        // Outermost of the diagnostic core so the matched route is populated by
        // the time the layer records the response, giving the `endpoint` label its
        // low-cardinality `MatchedPath` value rather than the raw URI.
        router = router.layer(http_metric_layer());
        if let Some(cors) = &self.cors {
            router = router.layer(cors.clone());
        }
        if let Some(csrf) = &self.csrf {
            router = router.layer(csrf.clone());
        }
        NormalizePathLayer::trim_trailing_slash().layer(router)
    }

    /// Apply the bare management-plane middleware and normalize paths before routing.
    ///
    /// The management plane serves observability endpoints to operators and
    /// scrapers, so it carries only the shared diagnostic layers and path
    /// normalization -- no CORS, CSRF, helmet, compression, decompression,
    /// server-timing, tenant echo, or request-metric layer. A scraper or kubelet
    /// therefore reaches `/metrics`, `/livez` and `/readyz` unencumbered, and the
    /// endpoints keep answering while the data plane is saturated or draining.
    pub fn apply_management(&self, router: Router) -> NormalizePath<Router> {
        let router = self.apply_diagnostic_layers(router);
        NormalizePathLayer::trim_trailing_slash().layer(router)
    }
}

#[cfg(test)]
mod content_coding_tests;

#[cfg(test)]
mod revalidation_tests;

#[cfg(test)]
mod tests {
    use crate::{
        config::{
            HttpStackConfig,
            compression::{CompressionConfig, CompressionQuality},
            cors::{CorsConfig, CorsMode},
            csrf::CsrfConfig,
            decompression::DecompressionConfig,
            feature_mode::HttpFeatureMode,
            helmet::HelmetConfig,
            server_timing::{ServerTimingConfig, ServerTimingPhase},
        },
        stack::HttpStack,
    };
    use andromeda_configuration_scalars::{human_byte_size::HumanByteSize, human_duration::HumanDuration};
    use andromeda_http_extraction::{blocking_decode::BlockingDecodePolicy, request_json_value_limit::RequestJsonValueLimit};
    use andromeda_http_headers::{forwarded_header_trust::ForwardedHeaderTrust, link::field_layout::LinkFieldLayout};
    use axum::{
        Extension,
        Router,
        body::{Body, to_bytes},
        extract::{Request, State},
        http::{HeaderMap, HeaderValue, StatusCode, header},
        middleware::{Next, from_fn_with_state},
        response::Response,
        routing::get,
    };
    use enumset::EnumSet;
    use std::{error::Error, num::NonZeroUsize, sync::Arc};
    use tokio::sync::Mutex;
    use tower::{Layer, ServiceExt};

    /// A stack configuration with every optional feature switched off.
    fn disabled_config() -> HttpStackConfig {
        HttpStackConfig {
            request_timeout: HumanDuration::from_secs(30),
            max_request_body: HumanByteSize::kib(1),
            max_request_json_values: NonZeroUsize::MIN.saturating_add(99),
            blocking_decode_threshold: HumanByteSize::kib(256),
            max_concurrent_blocking_decodes: NonZeroUsize::MIN,
            forwarded_headers: ForwardedHeaderTrust::Untrusted,
            link_field: LinkFieldLayout::Combined,
            cors: CorsConfig {
                mode: CorsMode::Disabled,
                ..CorsConfig::default()
            },
            compression: CompressionConfig {
                algorithms: EnumSet::empty(),
                quality: CompressionQuality::Default,
            },
            decompression: DecompressionConfig {
                algorithms: EnumSet::empty(),
                pass_through_unaccepted: false,
            },
            server_timing: ServerTimingConfig {
                mode: HttpFeatureMode::Disabled,
                phases: ServerTimingPhase::Total.into(),
            },
            helmet: HelmetConfig {
                mode: HttpFeatureMode::Disabled,
            },
            csrf: CsrfConfig {
                mode: HttpFeatureMode::Disabled,
                trusted_origins: Vec::new(),
            },
        }
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum Phase {
        Outer,
        Api,
        Tenant,
        Local,
        Handler,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum Direction {
        Inbound,
        Outbound,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct Event {
        phase: Phase,
        direction: Direction,
    }

    #[derive(Clone)]
    struct Marker {
        phase: Phase,
        events: Arc<Mutex<Vec<Event>>>,
    }

    async fn mark(State(marker): State<Marker>, request: Request, next: Next) -> Response {
        marker.events.lock().await.push(Event {
            phase: marker.phase,
            direction: Direction::Inbound,
        });
        let response = next.run(request).await;
        marker.events.lock().await.push(Event {
            phase: marker.phase,
            direction: Direction::Outbound,
        });
        response
    }

    fn marker(phase: Phase, events: &Arc<Mutex<Vec<Event>>>) -> Marker {
        Marker {
            phase,
            events: Arc::clone(events),
        }
    }

    #[tokio::test]
    async fn scope_markers_run_in_declared_inbound_order_and_reverse_outbound_order() -> Result<(), Box<dyn Error>> {
        let events = Arc::new(Mutex::new(Vec::new()));
        let handler_events = Arc::clone(&events);
        let route = Router::new()
            .route(
                "/ordered",
                get(move || {
                    let events = Arc::clone(&handler_events);
                    async move {
                        events.lock().await.push(Event {
                            phase: Phase::Handler,
                            direction: Direction::Inbound,
                        });
                        StatusCode::OK
                    }
                }),
            )
            .route_layer(from_fn_with_state(marker(Phase::Local, &events), mark))
            .layer(from_fn_with_state(marker(Phase::Tenant, &events), mark))
            .layer(from_fn_with_state(marker(Phase::Api, &events), mark));
        let service = from_fn_with_state(marker(Phase::Outer, &events), mark).layer(route);
        service.oneshot(Request::get("/ordered").body(Body::empty())?).await?;
        pretty_assertions::assert_eq!(
            *events.lock().await,
            vec![
                Event {
                    phase: Phase::Outer,
                    direction: Direction::Inbound
                },
                Event {
                    phase: Phase::Api,
                    direction: Direction::Inbound
                },
                Event {
                    phase: Phase::Tenant,
                    direction: Direction::Inbound
                },
                Event {
                    phase: Phase::Local,
                    direction: Direction::Inbound
                },
                Event {
                    phase: Phase::Handler,
                    direction: Direction::Inbound
                },
                Event {
                    phase: Phase::Local,
                    direction: Direction::Outbound
                },
                Event {
                    phase: Phase::Tenant,
                    direction: Direction::Outbound
                },
                Event {
                    phase: Phase::Api,
                    direction: Direction::Outbound
                },
                Event {
                    phase: Phase::Outer,
                    direction: Direction::Outbound
                },
            ]
        );
        Ok(())
    }

    #[tokio::test]
    async fn route_local_policy_does_not_affect_unrelated_routes() -> Result<(), Box<dyn Error>> {
        let events = Arc::new(Mutex::new(Vec::new()));
        let strict = Router::new()
            .route("/strict", get(|| async { StatusCode::OK }))
            .route_layer(from_fn_with_state(marker(Phase::Local, &events), mark));
        let router = Router::new()
            .route("/entity", get(|| async { StatusCode::OK }))
            .route("/health", get(|| async { StatusCode::OK }))
            .merge(strict)
            .layer(from_fn_with_state(marker(Phase::Api, &events), mark));
        for path in ["/entity", "/health"] {
            events.lock().await.clear();
            router.clone().oneshot(Request::get(path).body(Body::empty())?).await?;
            assert!(events.lock().await.iter().all(|event| event.phase != Phase::Local));
        }
        events.lock().await.clear();
        router.oneshot(Request::get("/strict").body(Body::empty())?).await?;
        assert!(events.lock().await.iter().any(|event| event.phase == Phase::Local));
        Ok(())
    }

    /// The API stack hands every route the operator's trust decision, so the
    /// origin extractor never has to guess it.
    #[tokio::test]
    async fn the_api_stack_installs_the_configured_forwarded_header_trust() -> Result<(), Box<dyn Error>> {
        for declared in [ForwardedHeaderTrust::Untrusted, ForwardedHeaderTrust::Trusted] {
            let stack = HttpStack::new(HttpStackConfig {
                forwarded_headers: declared,
                ..disabled_config()
            })?;
            let router = stack.apply_api(Router::new().route(
                "/probe",
                get(|Extension(trust): Extension<ForwardedHeaderTrust>| async move { trust.to_string() }),
            ));
            let response = router.oneshot(Request::get("/probe").body(Body::empty())?).await?;
            pretty_assertions::assert_eq!(response.status(), StatusCode::OK);
            let body = to_bytes(response.into_body(), usize::MAX).await?;
            pretty_assertions::assert_eq!(String::from_utf8(body.to_vec())?, declared.to_string());
        }
        Ok(())
    }

    /// A handler writes its `@context` link and its pagination links on a line
    /// each; the API stack lays them out as configured, whichever handler wrote
    /// them.
    #[tokio::test]
    async fn the_api_stack_lays_out_link_fields_as_configured() -> Result<(), Box<dyn Error>> {
        const CONTEXT: &str = r#"<https://example.org/ctx.jsonld>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json""#;
        const NEXT: &str = r#"</probe?limit=1&offset=1>; rel="next"; type="application/json""#;
        for (layout, expected) in [
            (LinkFieldLayout::Combined, vec![format!("{CONTEXT}, {NEXT}")]),
            (LinkFieldLayout::Grouped, vec![CONTEXT.to_owned(), NEXT.to_owned()]),
            (LinkFieldLayout::Separate, vec![CONTEXT.to_owned(), NEXT.to_owned()]),
        ] {
            let stack = HttpStack::new(HttpStackConfig {
                link_field: layout,
                ..disabled_config()
            })?;
            let router = stack.apply_api(Router::new().route(
                "/probe",
                get(|| async {
                    let mut headers = HeaderMap::new();
                    headers.append(header::LINK, HeaderValue::from_static(NEXT));
                    headers.append(header::LINK, HeaderValue::from_static(CONTEXT));
                    headers
                }),
            ));
            let response = router.oneshot(Request::get("/probe").body(Body::empty())?).await?;
            let lines = response
                .headers()
                .get_all(header::LINK)
                .iter()
                .map(|line| line.to_str().map(str::to_owned))
                .collect::<Result<Vec<_>, _>>()?;
            pretty_assertions::assert_eq!(lines, expected, "{layout}");
        }
        Ok(())
    }

    /// Every payload extractor reads the value limit and the decode threshold
    /// from the request, so the API stack has to publish both as configured.
    #[tokio::test]
    async fn the_stack_publishes_the_json_value_limit_and_the_decode_threshold() -> Result<(), Box<dyn Error>> {
        let stack = HttpStack::new(HttpStackConfig {
            max_request_json_values: NonZeroUsize::MIN.saturating_add(41),
            blocking_decode_threshold: HumanByteSize::kib(2),
            ..disabled_config()
        })?;
        let router = stack.apply_api(Router::new().route(
            "/probe",
            get(
                |Extension(limit): Extension<RequestJsonValueLimit>, Extension(policy): Extension<BlockingDecodePolicy>| async move {
                    format!("{} {}", limit.limit(), policy.threshold_bytes())
                },
            ),
        ));
        let response = router.oneshot(Request::get("/probe").body(Body::empty())?).await?;
        pretty_assertions::assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await?;
        pretty_assertions::assert_eq!(String::from_utf8(body.to_vec())?, "42 2048");
        Ok(())
    }

    /// The API subtree carries the security headers when helmet is enabled.
    #[tokio::test]
    async fn helmet_layer_is_installed_on_the_api_subtree() -> Result<(), Box<dyn Error>> {
        let stack = HttpStack::new(HttpStackConfig {
            helmet: HelmetConfig {
                mode: HttpFeatureMode::Enabled,
            },
            ..disabled_config()
        })?;
        let router = stack.apply_api(Router::new().route("/probe", get(|| async { StatusCode::OK })));
        let response = router.oneshot(Request::get("/probe").body(Body::empty())?).await?;
        assert!(
            response.headers().contains_key("x-content-type-options"),
            "expected a helmet security header, found {:?}",
            response.headers()
        );
        Ok(())
    }

    #[tokio::test]
    async fn process_stack_normalizes_paths_before_route_matching() -> Result<(), Box<dyn Error>> {
        let stack = HttpStack::new(disabled_config())?;
        let service = stack.apply_process(Router::new().route("/health", get(|| async { StatusCode::OK })));
        let response = service.oneshot(Request::get("/health/").body(Body::empty())?).await?;
        pretty_assertions::assert_eq!(response.status(), StatusCode::OK);
        Ok(())
    }

    /// The bare management stack answers without the edge-policy headers the
    /// process stack applies, while the process stack still emits them.
    #[tokio::test]
    async fn apply_management_installs_no_cors_csrf_or_helmet() -> Result<(), Box<dyn Error>> {
        let stack = HttpStack::new(HttpStackConfig {
            cors: CorsConfig {
                mode: CorsMode::Permissive,
                ..CorsConfig::default()
            },
            ..disabled_config()
        })?;
        let route = || Router::new().route("/livez", get(|| async { StatusCode::OK }));

        let management = stack.apply_management(route());
        let management_response = management
            .oneshot(Request::get("/livez").header("origin", "https://scraper.example").body(Body::empty())?)
            .await?;
        pretty_assertions::assert_eq!(management_response.status(), StatusCode::OK);
        assert!(
            !management_response.headers().contains_key("access-control-allow-origin"),
            "the bare management stack must not answer with a CORS header, found {:?}",
            management_response.headers()
        );
        assert!(
            !management_response.headers().contains_key("x-content-type-options"),
            "the bare management stack must not answer with a helmet header, found {:?}",
            management_response.headers()
        );

        let process = stack.apply_process(route());
        let process_response = process
            .oneshot(Request::get("/livez").header("origin", "https://scraper.example").body(Body::empty())?)
            .await?;
        assert!(
            process_response.headers().contains_key("access-control-allow-origin"),
            "the process stack still applies CORS, found {:?}",
            process_response.headers()
        );
        Ok(())
    }
}
