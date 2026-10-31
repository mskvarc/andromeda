//! Add context declaring text plain or no content type is 415.

use andromeda_test_suite::{broker, namespace::Namespace, problem::answered, wire::content_type};
use googletest::prelude::*;
use http::{HeaderName, StatusCode, header::CONTENT_TYPE};
use mediatype::{MediaType, media_type};
use ngsi_ld::{
    http::operation::AddContext,
    model::{Context, ContextEntry, InlineContext},
};

/// The media type the refused body is sent as.
const TEXT_PLAIN: MediaType<'static> = media_type!(TEXT / PLAIN);

/// Pins: Add @context (POST) declaring text/plain, or no Content-Type, is refused with a bare 415 and answers no
/// Location
/// Clause: 6.3.4, 5.13.2
/// Vocabulary: none (the test is about the media type of an @context write)
#[gtest]
#[tokio::test]
async fn n_cmn_cnt_9015_add_context_declaring_text_plain_or_no_content_type_is_415() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let context = Context::Single(ContextEntry::Inline(
        format!(r#"{{"Device_{namespace}": "https://test.andromeda/{namespace}/Device"}}"#).parse::<InlineContext>()?,
    ));
    let addition = AddContext::builder().context(&context).build();
    // The body goes as text/plain, which no typed operation sends, so the media type is replaced.
    let request = content_type(broker.unchecked(&addition)?, &TEXT_PLAIN)?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(
        &answer,
        answered(StatusCode::UNSUPPORTED_MEDIA_TYPE)
            .without_body()
            .without_header(HeaderName::from_static("location"))
    );
    // The body goes without a Content-Type, which no typed operation sends, so the media type is removed.
    let request_2 = broker.unchecked(&addition)?.remove_header(&CONTENT_TYPE);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(
        &answer_2,
        answered(StatusCode::UNSUPPORTED_MEDIA_TYPE)
            .without_body()
            .without_header(HeaderName::from_static("location"))
    );
    Ok(())
}
