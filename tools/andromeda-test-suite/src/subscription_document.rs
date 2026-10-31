//! A Subscription body carrying members of its `notification` the model cannot write (`pick`, `omit`), for a test
//! whose subject is the projection a notification applies (clause 4.21, Table 5.2.14-1).
//!
//! The subscription is built and rendered by the model, so everything the model writes stays typed; only the members
//! it lacks are added to the rendered `notification` object, as JSON text.

use crate::{
    broker::BrokerClient,
    unchecked_document::{UncheckedDocumentError, from_rendered},
};
use ngsi_ld::{
    client::{ClientError, ReqwestError},
    http::{UncheckedDocument, UncheckedRequest, operation::CreateSubscription},
    model::{Input, InvalidJson, RawJson},
    subscription::Subscription,
};
use serde_json::{Map, Value};

/// Why a subscription body with extra notification members could not be built.
#[derive(Debug, thiserror::Error)]
pub enum SubscriptionDocumentError {
    /// The rendering is not a JSON object.
    #[error("the rendered subscription is not a JSON object: {0}")]
    NotAnObject(#[source] serde_json::Error),
    /// The rendering has no `notification` object.
    #[error("the rendered subscription has no notification object")]
    NoNotification,
    /// A member the test adds is not JSON.
    #[error("a notification member is not JSON")]
    Member(#[source] serde_json::Error),
    /// The edited document cannot be written back.
    #[error("the edited subscription cannot be written")]
    Write(#[source] serde_json::Error),
    /// The written document does not read back as JSON.
    #[error("the edited subscription does not read back as JSON")]
    Reread(#[source] InvalidJson),
    /// The edited document is not an unchecked document.
    #[error(transparent)]
    Unchecked(#[from] UncheckedDocumentError),
    /// The client refused to build the Create Subscription request.
    #[error("the request could not be built")]
    Request(#[source] Box<ClientError<ReqwestError>>),
}

/// The document of `rendered`, a subscription the model wrote, with each of `members` (a name and its JSON text) set
/// in its `notification` object.
///
/// # Errors
///
/// Returns [`SubscriptionDocumentError`] when the rendering has no `notification` object or a member is not JSON.
pub fn with_notification_members(rendered: &RawJson, members: &[(&str, &str)]) -> Result<UncheckedDocument, SubscriptionDocumentError> {
    let mut document: Map<String, Value> = serde_json::from_str(&rendered.to_string()).map_err(SubscriptionDocumentError::NotAnObject)?;
    let notification = document
        .get_mut("notification")
        .and_then(Value::as_object_mut)
        .ok_or(SubscriptionDocumentError::NoNotification)?;
    members.iter().try_for_each(|(name, text)| {
        let value = serde_json::from_str(text).map_err(SubscriptionDocumentError::Member)?;
        notification.insert((*name).to_owned(), value);
        Ok::<(), SubscriptionDocumentError>(())
    })?;
    let written = serde_json::to_string(&document).map_err(SubscriptionDocumentError::Write)?;
    let edited: RawJson = written.parse().map_err(SubscriptionDocumentError::Reread)?;
    Ok(from_rendered(&edited)?)
}

/// The Create Subscription request for `subscription` with `members` added to its `notification` object, to send with
/// `send_unchecked`.
///
/// # Errors
///
/// Returns [`SubscriptionDocumentError`] when the document cannot be built or the client refuses the request.
pub fn create_request(
    client: &BrokerClient,
    subscription: &Subscription<Input>,
    members: &[(&str, &str)],
) -> Result<UncheckedRequest, SubscriptionDocumentError> {
    let document = with_notification_members(&subscription.render(), members)?;
    let request = client
        .unchecked(&CreateSubscription { subscription })
        .map_err(|error| SubscriptionDocumentError::Request(Box::new(error)))?;
    Ok(request.document(&document))
}

#[cfg(test)]
mod tests {
    use crate::subscription_document::with_notification_members;
    use googletest::prelude::*;
    use ngsi_ld::model::RawJson;

    #[gtest]
    fn members_are_added_to_the_notification_object() -> Result<()> {
        let rendered: RawJson =
            r#"{"id":"urn:ngsi-ld:Subscription:1","type":"Subscription","notification":{"endpoint":{"uri":"http://sink.test/"}}}"#.parse()?;
        let document = with_notification_members(&rendered, &[("pick", r#"["id","owner{name}"]"#), ("join", r#""flat""#)])?;
        verify_that!(
            document.to_string(),
            all!(
                contains_substring(r#""pick":["id","owner{name}"]"#),
                contains_substring(r#""join":"flat""#),
                contains_substring("endpoint")
            )
        )
    }

    #[gtest]
    fn a_rendering_without_a_notification_object_is_refused() -> Result<()> {
        let rendered: RawJson = r#"{"id":"urn:ngsi-ld:Subscription:1"}"#.parse()?;
        verify_that!(with_notification_members(&rendered, &[("pick", "[]")]), err(anything()))
    }

    #[gtest]
    fn a_member_that_is_not_json_is_refused() -> Result<()> {
        let rendered: RawJson = r#"{"notification":{}}"#.parse()?;
        verify_that!(with_notification_members(&rendered, &[("pick", "[")]), err(anything()))
    }
}
