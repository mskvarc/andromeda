//! Deterministic application of API-wide and process-wide middleware.

mod body_arrival;
mod compression_layer;
mod cors_layer;
pub mod csrf_layer;
mod decompression_layer;
mod encoded_entity_tag;
mod forwarded_trust_layer;
pub mod helmet_layer;
mod request_deadline;
mod request_metrics;
mod server_timing;
mod strict_transport_security_layer;

use crate::{
    cors_tokens::CorsTokenError,
    stack::{
        compression_layer::build_compression_layer,
        cors_layer::build_cors_layer,
        csrf_layer::{CsrfLayerError, build_csrf_layer},
        decompression_layer::{build_decompression_layer, record_received_content_length},
        encoded_entity_tag::EncodedEntityTags,
        forwarded_trust_layer::{ForwardedTrustPolicy, install_forwarded_trust},
        helmet_layer::{SecurityHeadersError, build_helmet_layer},
        request_deadline::{RequestDeadline, answer_within_deadline},
        request_metrics::observe_request,
        server_timing::apply_server_timing,
        strict_transport_security_layer::{set_strict_transport_security, strict_transport_security},
    },
};
use andromeda_configuration::schema::{
    http::{HttpStackConfig, body_decoding::BodyDecodingConfigError, server_timing::ServerTimingConfig},
    link_field_layout::LinkFieldLayout,
    request_limits::{RequestLimitsConfig, RequestLimitsConfigError},
};
use andromeda_http_extraction::{blocking_decode::BlockingDecodePolicy, request_body_limit::RequestBodyLimit, request_json_value_limit::RequestJsonValueLimit};
use andromeda_http_headers::{link::field_layout::apply_link_field_layout, ngsild_tenant::NgsiLdTenant, x_request_id::XRequestId};
use andromeda_http_tenancy::propagation::echo_tenant_header;
use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
use axum::{
    Extension,
    Router,
    extract::{DefaultBodyLimit, MatchedPath, Request},
    http::{HeaderMap, header},
    middleware,
    response::Response,
};
use axum_helmet::HelmetLayer;
use headers::{Header, HeaderMapExt, StrictTransportSecurity};
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
    trace::TraceLayer,
};

/// A fallible middleware layer could not be built from its configuration.
#[derive(Debug, Error)]
pub enum HttpStackBuildError {
    /// The body ceiling does not fit the platform address width.
    #[error(transparent)]
    RequestLimits(#[from] RequestLimitsConfigError),
    /// The blocking-decode threshold does not fit the platform address width.
    #[error(transparent)]
    BodyDecoding(#[from] BodyDecodingConfigError),
    /// A configured security field cannot be rendered.
    #[error(transparent)]
    SecurityHeaders(#[from] SecurityHeadersError),
    /// A CSRF trusted origin could not be admitted.
    #[error(transparent)]
    Csrf(#[from] CsrfLayerError),
    /// A CORS method or header name could not be read.
    #[error(transparent)]
    Cors(#[from] CorsTokenError),
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
    /// Whose forwarded fields are believed.
    forwarded_trust: ForwardedTrustPolicy,
    /// How a response's link-values are laid out over `Link` field lines.
    link_field: LinkFieldLayout,
    /// Maximum wall-clock time for one HTTP request.
    request_deadline: RequestDeadline,
    /// `Server-Timing` policy, applied per assembled router.
    server_timing: ServerTimingConfig,
    /// The prebuilt security-header layer, absent when disabled.
    helmet: Option<HelmetLayer>,
    /// The `Strict-Transport-Security` policy sent over TLS, absent when
    /// disabled.
    strict_transport_security: Option<StrictTransportSecurity>,
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
    /// Construct the HTTP stack `config` describes, enforcing `limits` on every
    /// request.
    ///
    /// # Errors
    ///
    /// Returns [`HttpStackBuildError`] when a configured size does not fit the
    /// platform address width, the security-header layer cannot be rendered or a
    /// CSRF trusted origin is not one the CSRF layer accepts.
    pub fn new(config: HttpStackConfig, limits: RequestLimitsConfig) -> Result<HttpStack, HttpStackBuildError> {
        let request_body_limit = RequestBodyLimit::new(limits.max_body_bytes()?);
        let blocking_decode = BlockingDecodePolicy::new(config.body_decoding.blocking_threshold_bytes()?, config.body_decoding.max_concurrent_blocking);
        Ok(HttpStack {
            request_body_limit,
            request_json_value_limit: RequestJsonValueLimit::new(JsonValueLimit::new(limits.max_json_values)),
            blocking_decode,
            forwarded_trust: ForwardedTrustPolicy::from(&config.forwarded),
            link_field: config.link_field,
            request_deadline: RequestDeadline::new(limits.timeout.duration()),
            helmet: build_helmet_layer(&config.security_headers)?,
            strict_transport_security: strict_transport_security(&config.security_headers),
            cors: build_cors_layer(&config.cors)?,
            compression: build_compression_layer(&config.compression),
            encoded_entity_tags: EncodedEntityTags::new(config.compression.active_algorithms()),
            decompression: build_decompression_layer(&config.decompression),
            csrf: build_csrf_layer(&config.csrf)?,
            server_timing: config.server_timing,
        })
    }

    /// Apply middleware scoped to the complete NGSI-LD API subtree.
    pub fn apply_api(&self, router: Router) -> Router {
        // Every absolute URL the broker mints from a request origin reads the
        // trust decided here for the request's peer, so a router assembled
        // without the stack treats every forwarded field as untrusted rather
        // than inheriting a permissive default.
        let mut router = router
            .layer(middleware::from_fn_with_state(self.forwarded_trust.clone(), install_forwarded_trust)) // clone: the middleware owns its copy of the policy
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
        if let Some(policy) = &self.strict_transport_security {
            router = router.layer(middleware::from_fn_with_state(policy.clone(), set_strict_transport_security)); // clone: the middleware owns its copy of the policy
        }
        // Applied to the assembled response, so the `@context` link and the
        // pagination links each handler writes are laid out once, whichever
        // handler wrote them.
        let link_field = self.link_field;
        router.layer(middleware::map_response(move |mut response: Response| async move {
            apply_link_field_layout(link_field, response.headers_mut());
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
            .layer(middleware::from_fn_with_state(self.request_deadline, answer_within_deadline));
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
        // Outside the diagnostic core, so a request the deadline or the body
        // limits answer is counted with the status it was answered with.
        // `Router::layer` runs it after routing, which gives the `endpoint`
        // label the matched route template rather than the raw path.
        router = router.layer(middleware::from_fn(observe_request));
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
    use crate::stack::HttpStack;
    use andromeda_configuration::{
        human_byte_size::HumanByteSize,
        human_duration::HumanDuration,
        schema::{
            feature_mode::FeatureMode,
            http::{
                HttpStackConfig,
                body_decoding::BodyDecodingConfig,
                compression::{CompressionConfig, CompressionQuality},
                cors::{CorsConfig, CorsMode},
                csrf::CsrfConfig,
                decompression::{DecompressionConfig, UnacceptedEncoding},
                forwarded::{ForwardedConfig, ForwardedTrust},
                security_headers::SecurityHeadersConfig,
                server_timing::{ServerTimingConfig, ServerTimingPhase},
            },
            link_field_layout::LinkFieldLayout,
            request_limits::RequestLimitsConfig,
        },
    };
    use andromeda_http_extraction::{
        blocking_decode::BlockingDecodePolicy,
        forwarded_field_trust::ForwardedFieldTrust,
        request_json_value_limit::RequestJsonValueLimit,
    };
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
            body_decoding: BodyDecodingConfig {
                blocking_threshold: HumanByteSize::kib(256),
                max_concurrent_blocking: NonZeroUsize::MIN,
            },
            link_field: LinkFieldLayout::Combined,
            cors: CorsConfig {
                mode: CorsMode::Disabled,
                ..CorsConfig::default()
            },
            compression: CompressionConfig {
                mode: FeatureMode::Disabled,
                algorithms: EnumSet::all(),
                quality: CompressionQuality::Default,
            },
            decompression: DecompressionConfig {
                mode: FeatureMode::Disabled,
                algorithms: EnumSet::all(),
                unaccepted_encoding: UnacceptedEncoding::Refused,
            },
            forwarded: ForwardedConfig::default(),
            server_timing: ServerTimingConfig {
                mode: FeatureMode::Disabled,
                phases: ServerTimingPhase::Total.into(),
            },
            security_headers: SecurityHeadersConfig {
                mode: FeatureMode::Disabled,
                ..SecurityHeadersConfig::default()
            },
            csrf: CsrfConfig {
                mode: FeatureMode::Disabled,
                trusted_origins: Vec::new(),
            },
        }
    }

    /// The request limits the tests run under: a 1 KiB body of at most 100
    /// JSON values.
    fn test_limits() -> RequestLimitsConfig {
        RequestLimitsConfig {
            timeout: HumanDuration::from_secs(30),
            max_body: HumanByteSize::kib(1),
            max_json_values: NonZeroUsize::MIN.saturating_add(99),
            ..RequestLimitsConfig::default()
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

    /// The API stack hands every route the trust decided for its peer, so the
    /// origin extractor never has to guess it.
    #[tokio::test]
    async fn the_api_stack_installs_the_configured_forwarded_trust() -> Result<(), Box<dyn Error>> {
        for (trust, expected) in [
            (ForwardedTrust::None, ForwardedFieldTrust::Untrusted),
            (ForwardedTrust::Any, ForwardedFieldTrust::Trusted),
        ] {
            let stack = HttpStack::new(
                HttpStackConfig {
                    forwarded: ForwardedConfig { trust, proxies: Vec::new() },
                    ..disabled_config()
                },
                test_limits(),
            )?;
            let router = stack.apply_api(Router::new().route(
                "/probe",
                get(|Extension(trust): Extension<ForwardedFieldTrust>| async move { format!("{trust:?}") }),
            ));
            let response = router.oneshot(Request::get("/probe").body(Body::empty())?).await?;
            pretty_assertions::assert_eq!(response.status(), StatusCode::OK);
            let body = to_bytes(response.into_body(), usize::MAX).await?;
            pretty_assertions::assert_eq!(String::from_utf8(body.to_vec())?, format!("{expected:?}"));
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
            let stack = HttpStack::new(
                HttpStackConfig {
                    link_field: layout,
                    ..disabled_config()
                },
                test_limits(),
            )?;
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
        let stack = HttpStack::new(
            HttpStackConfig {
                body_decoding: BodyDecodingConfig {
                    blocking_threshold: HumanByteSize::kib(2),
                    max_concurrent_blocking: NonZeroUsize::MIN,
                },
                ..disabled_config()
            },
            RequestLimitsConfig {
                max_json_values: NonZeroUsize::MIN.saturating_add(41),
                ..test_limits()
            },
        )?;
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
        let stack = HttpStack::new(
            HttpStackConfig {
                security_headers: SecurityHeadersConfig::default(),
                ..disabled_config()
            },
            test_limits(),
        )?;
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
        let stack = HttpStack::new(disabled_config(), test_limits())?;
        let service = stack.apply_process(Router::new().route("/health", get(|| async { StatusCode::OK })));
        let response = service.oneshot(Request::get("/health/").body(Body::empty())?).await?;
        pretty_assertions::assert_eq!(response.status(), StatusCode::OK);
        Ok(())
    }

    /// The bare management stack answers without the edge-policy headers the
    /// process stack applies, while the process stack still emits them.
    #[tokio::test]
    async fn apply_management_installs_no_cors_csrf_or_helmet() -> Result<(), Box<dyn Error>> {
        let stack = HttpStack::new(
            HttpStackConfig {
                cors: CorsConfig {
                    mode: CorsMode::Permissive,
                    ..CorsConfig::default()
                },
                ..disabled_config()
            },
            test_limits(),
        )?;
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
