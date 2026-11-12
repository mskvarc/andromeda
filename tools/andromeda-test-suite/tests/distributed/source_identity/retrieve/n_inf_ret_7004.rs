//! Every alias the broker answers is an HTTP token.

use andromeda_test_suite::{broker, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{http::headers::Accept, model::NgsiLdMediaType};

/// The `tchar` of IETF RFC 7230 section 3.2.6 beside letters and digits.
const TOKEN_SYMBOLS: &str = "!#$%&'*+-.^_`|~";

/// Pins: Retrieve Context Source Identity Information answers the configured `broker-a` as the default tenant's
/// `contextSourceAlias`, and for a named tenant a non-empty `contextSourceAlias` made only of the `tchar` of IETF
/// RFC 7230
/// Clause: 5.2.40, 4.14, 5.15.1.4
/// Tenant: private (the alias of a named tenant is asked of a tenant of its own, and a tenant comes into existence
/// only by a write that targets it)
#[gtest]
#[tokio::test]
async fn n_inf_ret_7004_every_alias_the_broker_answers_is_an_http_token() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let identity = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).source_identity().send().await?;
    expect_that!(identity.alias.to_string(), eq("broker-a"));
    let identity_2 = tenant.with_accept(Accept::new(NgsiLdMediaType::Json)).source_identity().send().await?;
    // Table 5.2.40-1 makes the alias an IETF RFC 7230 pseudonym, which is a token: one or more `tchar`.
    expect_that!(
        identity_2.alias.to_string().chars().collect::<Vec<_>>(),
        all!(
            not(is_empty()),
            each(predicate(
                |character: &char| character.is_ascii_alphanumeric() || TOKEN_SYMBOLS.contains(*character)
            ))
        )
    );
    Ok(())
}
