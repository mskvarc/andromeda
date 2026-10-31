//! Header fields appended to an unchecked request exactly as a test writes them, for a request whose subject is a
//! field the typed headers cannot form (a `Forwarded` field that is no RFC 7239 element, `X-Forwarded-Proto`).

use http::{
    HeaderName,
    HeaderValue,
    header::{InvalidHeaderName, InvalidHeaderValue},
};
use ngsi_ld::http::UncheckedRequest;

/// Why a field cannot be written.
#[derive(Debug, thiserror::Error)]
pub enum FieldError {
    /// The name is not a header field name.
    #[error("a field name is not a header field name")]
    Name(#[source] InvalidHeaderName),
    /// The value is not a header field value.
    #[error("a field value is not a header field value")]
    Value(#[source] InvalidHeaderValue),
}

/// `request` with the fields `fields` appended in order, each as given.
///
/// # Errors
///
/// Returns [`FieldError`] when a name or a value cannot be written as a header field.
pub fn with_fields(request: UncheckedRequest, fields: &[(&str, &str)]) -> Result<UncheckedRequest, FieldError> {
    fields.iter().try_fold(request, |request, (name, value)| {
        let field = HeaderName::from_bytes(name.as_bytes()).map_err(FieldError::Name)?;
        let text = HeaderValue::from_str(value).map_err(FieldError::Value)?;
        Ok(request.append_header(field, text))
    })
}

#[cfg(test)]
mod tests {
    use crate::{broker::client_at, request_fields::with_fields, wire::request_to};
    use googletest::prelude::*;
    use http::Method;
    use url::Url;

    #[gtest]
    fn fields_are_appended_and_an_unwritable_one_is_refused() -> Result<()> {
        let client = client_at(Url::parse("http://localhost:1/ngsi-ld/v1/")?)?;
        let request = request_to(&client, Method::GET, "entities")?;
        expect_that!(
            with_fields(request, &[("Forwarded", "proto=https"), ("X-Forwarded-Proto", "https")]).is_ok(),
            eq(true)
        );
        let request_2 = request_to(&client, Method::GET, "entities")?;
        expect_that!(with_fields(request_2, &[("Forwarded", "line\nbreak")]).is_err(), eq(true));
        let request_3 = request_to(&client, Method::GET, "entities")?;
        verify_that!(with_fields(request_3, &[("bad name", "x")]).is_err(), eq(true))
    }
}
