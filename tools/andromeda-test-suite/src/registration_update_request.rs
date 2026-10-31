//! Update Context Source Registration with the fragment as rendered but for its `type` member, which the typed
//! fragment always writes and clause 5.9.3 does not require of an update.

use crate::broker::BrokerClient;
use ngsi_ld::{
    client::{ClientError, ReqwestError},
    http::{NotAnObject, UncheckedDocument, UncheckedRequest, operation::UpdateRegistration},
    registration::{RegistrationFragment, RegistrationId},
};

/// Why the request could not be written.
#[derive(Debug, thiserror::Error)]
pub enum UpdateRequestError {
    /// The rendered fragment is not a JSON object.
    #[error("the fragment is not a JSON object")]
    NotAnObject(#[from] NotAnObject),
    /// The client refused to write the request.
    #[error("the request could not be written")]
    Request(#[source] Box<ClientError<ReqwestError>>),
}

/// The Update request for registration `id` carrying `fragment` as rendered but for its `type` member.
///
/// # Errors
///
/// Returns [`UpdateRequestError`] when the fragment does not render as an object or the client cannot write the
/// request.
pub fn update_without_type(broker: &BrokerClient, id: &RegistrationId, fragment: &RegistrationFragment) -> Result<UncheckedRequest, UpdateRequestError> {
    let document = UncheckedDocument::try_from(fragment.render())?.remove_member("type");
    let request = broker
        .unchecked(&UpdateRegistration { id, fragment })
        .map_err(|error| UpdateRequestError::Request(Box::new(error)))?;
    Ok(request.document(&document))
}

#[cfg(test)]
mod tests {
    use crate::{broker::client_at, namespace::Namespace, registration_update_request::update_without_type};
    use googletest::prelude::*;
    use ngsi_ld::registration::RegistrationFragment;
    use url::Url;

    #[gtest]
    fn the_body_leaves_out_the_type_and_keeps_the_endpoint() -> Result<()> {
        let namespace = Namespace::fresh();
        let client = client_at(Url::parse("http://broker.test/ngsi-ld/v1/")?)?;
        let fragment = RegistrationFragment::builder().endpoint("http://source.test/moved").build()?;
        let request = update_without_type(&client, &namespace.registration_id(1)?, &fragment)?;
        let body = String::from_utf8(request.as_request().body().to_vec())?;
        expect_that!(
            body,
            all!(
                contains_substring("http://source.test/moved"),
                not(contains_substring("ContextSourceRegistration"))
            )
        );
        Ok(())
    }
}
