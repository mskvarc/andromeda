//! A `@context` stored through a TLS listener (Add @context, clause 6.29.3.1) is named by a URL with the https origin
//! the client used, and that URL serves it (clauses 5.13.2.4 and 6.2). The store, the listing of hosted contexts, the
//! serve of the minted URL and the removal run in the order a client would run them.

use crate::{
    body_json::json_equals,
    broker::BrokerClient,
    context_details::{list_details, metadata_list},
    context_record::segment_of_location,
    namespace::Namespace,
    request_fields::with_fields,
    wire::{content_type, request_to},
};
use googletest::prelude::*;
use http::{Method, StatusCode, Version, header::LOCATION};
use ngsi_ld::{
    http::operation::{AddContext, ContextKindFilter},
    model::{APPLICATION_JSON, Context, ContextEntry, InlineContext},
};
use url::Url;

/// Stores a context holding one term of `namespace` through `client` with `headers` added to the request, and requires
/// the exchange to be over `version`, the context to be listed under a URL on `origin`, that URL to serve the term,
/// and the context to be removable.
///
/// # Errors
///
/// Returns the error of a request that cannot be built or sent, or of an answer that cannot be read.
pub async fn expect_https_url(client: &BrokerClient, namespace: &Namespace, headers: &[(&str, &str)], version: Version, origin: &Url) -> Result<()> {
    let term = format!("tls{namespace}");
    let definition = format!("https://example.org/{namespace}/tls");
    let context = Context::Single(ContextEntry::Inline(format!(r#"{{"{term}": "{definition}"}}"#).parse::<InlineContext>()?));
    let request = content_type(client.unchecked(&AddContext::builder().context(&context).build())?, &APPLICATION_JSON)?;
    let created = client.send_unchecked(with_fields(request, headers)?).await?;
    expect_that!(created.status(), eq(StatusCode::CREATED));
    expect_that!(created.version(), eq(version));
    let location = created.headers().get(LOCATION).and_then(|value| value.to_str().ok()).or_fail()?;
    let local_id = segment_of_location(location)?;
    let listed = list_details(client, Some(ContextKindFilter::Hosted)).await?;
    expect_that!(listed.status(), eq(StatusCode::OK));
    let metadata = metadata_list(&listed)?;
    let stored = metadata.iter().find(|candidate| candidate.local_id.as_str() == local_id).or_fail()?;
    expect_that!(stored.url.as_str(), starts_with(format!("{origin}ngsi-ld/v1/jsonldContexts/")));
    let served = client
        .send_unchecked(request_to(client, Method::GET, "")?.uri(stored.url.as_str().parse()?))
        .await?;
    expect_that!(served.status(), eq(StatusCode::OK));
    expect_that!(served.body(), json_equals(&format!("$['@context']['{term}']"), definition));
    let removed = client
        .send_unchecked(request_to(client, Method::DELETE, &format!("jsonldContexts/{local_id}"))?)
        .await?;
    expect_that!(removed.status(), eq(StatusCode::NO_CONTENT));
    Ok(())
}
