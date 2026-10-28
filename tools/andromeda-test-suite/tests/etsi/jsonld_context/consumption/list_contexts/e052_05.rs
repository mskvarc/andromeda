//! ETSI `052_05`: a `details=true` listing of the stored `@context`s names each of the three contexts the case added,
//! and, of kind `Hosted`, describes each of them (clauses 5.13.3 and 5.13.3.5).

use crate::jsonld_context::stored_contexts::{add_initial_set, detailed_listing, expect_context_details, listed_entry};
use andromeda_test_suite::{broker, namespace::Namespace, teardown::AnyAnswer};
use googletest::prelude::*;
use ngsi_ld::http::operation::ContextKindFilter;

/// Pins: Check that one can list @contexts
/// Clause: 5.13.3
/// Changed: Check Response Reason set to compares the HTTP/1.1 reason phrase, which Hurl cannot read; RFC 9112
/// clause 4 has a client ignore it and HTTP/2 carries none (RFC 9113 clause 8.3.2), so the port checks the status code
/// the phrase names
/// Changed: the listing is required to be a JSON array (Table 6.29.3.2-2), where the keyword only parses and
/// iterates it
/// Changed: a listed URL is compared as the broker lists it, where the keyword percent-decodes it first: clause
/// 5.13.3.1 has a client use each listed URL as given
/// Tenant: private (the case looks its own @contexts up in a listing of the tenant, which in the shared tenant holds
/// every other file's entries; Andromeda answers at most the first 1000 entries of a kind in identifier order and drops
/// the rest (defect D-25), so the file's own entries are listed reliably only in a tenant of its own)
/// Vocabulary: none (the case is about @context storage itself and references no suite @context)
#[gtest]
#[tokio::test]
async fn e052_05_01_list_contexts_details_true_no_kind_several_added() -> Result<()> {
    let namespace = Namespace::fresh();
    let tenant = broker::private_tenant(&broker::front()?, &namespace).await?;
    let [first, second, third] = add_initial_set(&tenant, &namespace).await?;
    let listing = detailed_listing(&tenant, None).await?;
    expect_that!(listing.as_array(), some(anything()));
    expect_that!(listed_entry(&listing, &first.location), some(anything()));
    expect_that!(listed_entry(&listing, &second.location), some(anything()));
    expect_that!(listed_entry(&listing, &third.location), some(anything()));
    tenant.contexts().delete(&first.id).send().await.any_answer();
    tenant.contexts().delete(&second.id).send().await.any_answer();
    tenant.contexts().delete(&third.id).send().await.any_answer();
    Ok(())
}

/// Pins: Check that one can list @contexts
/// Clause: 5.13.3
/// Changed: Check Response Reason set to compares the HTTP/1.1 reason phrase, which Hurl cannot read; RFC 9112
/// clause 4 has a client ignore it and HTTP/2 carries none (RFC 9113 clause 8.3.2), so the port checks the status code
/// the phrase names
/// Changed: the listing is required to be a JSON array (Table 6.29.3.2-2), where the keyword only parses and
/// iterates it
/// Changed: a listed URL is compared as the broker lists it, where the keyword percent-decodes it first: clause
/// 5.13.3.1 has a client use each listed URL as given
/// Changed: lastUsage and numberOfHits are required to be absent or of their type (a `DateTime` and a number, clause
/// 5.13.3.5), where the keyword also skips a null or empty value
/// Tenant: private (the case looks its own @contexts up in a listing of the tenant, which in the shared tenant holds
/// every other file's entries; Andromeda answers at most the first 1000 entries of a kind in identifier order and drops
/// the rest (defect D-25), so the file's own entries are listed reliably only in a tenant of its own)
/// Vocabulary: none (the case is about @context storage itself and references no suite @context)
#[gtest]
#[tokio::test]
async fn e052_05_02_list_contexts_details_true_kind_hosted_several_added() -> Result<()> {
    let namespace = Namespace::fresh();
    let tenant = broker::private_tenant(&broker::front()?, &namespace).await?;
    let [first, second, third] = add_initial_set(&tenant, &namespace).await?;
    let listing = detailed_listing(&tenant, Some(ContextKindFilter::Hosted)).await?;
    expect_that!(listing.as_array(), some(anything()));
    let first_entry = listed_entry(&listing, &first.location).or_fail()?;
    expect_context_details(first_entry, ContextKindFilter::Hosted);
    let second_entry = listed_entry(&listing, &second.location).or_fail()?;
    expect_context_details(second_entry, ContextKindFilter::Hosted);
    let third_entry = listed_entry(&listing, &third.location).or_fail()?;
    expect_context_details(third_entry, ContextKindFilter::Hosted);
    tenant.contexts().delete(&first.id).send().await.any_answer();
    tenant.contexts().delete(&second.id).send().await.any_answer();
    tenant.contexts().delete(&third.id).send().await.any_answer();
    Ok(())
}
