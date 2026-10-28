//! A deliberately invalid request body built from a valid typed document: the document is rendered by the model and
//! every member of it carried into an [`UncheckedDocument`], on which the test then sets only the member the model
//! cannot write.
//!
//! The model builds an `UncheckedDocument` from an entity alone; subscriptions, registrations, temporal entities and
//! fragments reach one through their rendering.

use ngsi_ld::{http::UncheckedDocument, model::RawJson};

/// Why a rendering cannot become an unchecked document.
#[derive(Debug, thiserror::Error)]
pub enum UncheckedDocumentError {
    /// The rendering is not a JSON object.
    #[error("a rendered document is not a JSON object: {0}")]
    NotAnObject(String),
    /// A member of the rendering is not JSON text the model reads back.
    #[error("a rendered member is not JSON: {0}")]
    Member(String),
}

/// The unchecked document holding every member of `rendered`, a document the model wrote.
///
/// # Errors
///
/// Returns [`UncheckedDocumentError`] when `rendered` is not a JSON object.
pub fn from_rendered(rendered: &RawJson) -> Result<UncheckedDocument, UncheckedDocumentError> {
    let text = rendered.to_string();
    let members: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&text).map_err(|_| UncheckedDocumentError::NotAnObject(text.clone()))?;
    members.iter().try_fold(UncheckedDocument::empty(), |document, (name, value)| {
        let raw: RawJson = value.to_string().parse().map_err(|_| UncheckedDocumentError::Member(name.clone()))?;
        Ok(document.set_member(name, &raw))
    })
}

#[cfg(test)]
mod tests {
    use crate::unchecked_document::from_rendered;
    use googletest::prelude::*;
    use ngsi_ld::model::RawJson;

    #[gtest]
    fn every_member_of_the_rendering_is_carried() -> Result<()> {
        let rendered: RawJson = r#"{"type":"Subscription","q":"speed>50"}"#.parse()?;
        let invalid: RawJson = r#""speed>>""#.parse()?;
        let document = from_rendered(&rendered)?.set_member("q", &invalid);
        verify_that!(format!("{document:?}"), all!(contains_substring("Subscription"), contains_substring("speed>>")))
    }

    #[gtest]
    fn a_rendering_that_is_not_an_object_is_refused() -> Result<()> {
        let rendered: RawJson = "[1]".parse()?;
        verify_that!(from_rendered(&rendered), err(anything()))
    }
}
