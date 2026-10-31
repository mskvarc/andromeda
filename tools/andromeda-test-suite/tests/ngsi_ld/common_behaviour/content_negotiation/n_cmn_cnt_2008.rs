//! Add context declaring merge patch json is 415.

use andromeda_test_suite::{broker, namespace::Namespace, problem::answered, wire::content_type};
use googletest::prelude::*;
use http::{HeaderName, StatusCode};
use ngsi_ld::{
    http::{headers::ContentType, operation::AddContext},
    model::{Context, ContextEntry, InlineContext},
};

/// Pins: Add @context (POST) declaring application/merge-patch+json is refused with a bare 415 and answers no
/// Location
/// Clause: 6.3.4, 5.13.2
/// Recorded: 6.3.4 allows application/merge-patch+json for PATCH requests only; any other media type than
/// application/json and application/ld+json is 415 without a body
/// Vocabulary: none (the test is about the media type of an @context write)
#[gtest]
#[tokio::test]
async fn n_cmn_cnt_2008_add_context_declaring_merge_patch_json_is_415() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let context = Context::Single(ContextEntry::Inline(
        format!(r#"{{"Device_{namespace}": "https://test.andromeda/{namespace}/Device"}}"#).parse::<InlineContext>()?,
    ));
    // The body goes as application/merge-patch+json, which no typed operation sends, so the media type is replaced.
    let request = content_type(
        broker.unchecked(&AddContext::builder().context(&context).build())?,
        &ContentType::MergePatch.media_type(),
    )?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(
        &answer,
        answered(StatusCode::UNSUPPORTED_MEDIA_TYPE)
            .without_body()
            .without_header(HeaderName::from_static("location"))
    );
    Ok(())
}
