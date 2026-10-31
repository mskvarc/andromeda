//! An expanded type past the bound is refused and creates nothing.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, ContextEntry, Entity, ErrorType, InlineContext, NgsiLdMediaType},
};

/// Pins: Create Entity whose type is a short term an inline @context maps to an IRI of over 1,100 octets answers
/// 400 `BadRequestData`, and the Entity does not exist
/// Clause: 4.5.1, 5.6.1, 5.5.4
/// Recorded: ETSI GS CIM 009 sets no length on an identifier; Andromeda bounds every identifier it stores at 1,024
/// octets, the value as stored, which for a term is its expansion
#[gtest]
#[tokio::test]
async fn a_lim_idn_9002_an_expanded_type_past_the_bound_is_refused_and_creates_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let body_context = broker.with_context(Context::Multiple(vec![ContextEntry::Inline(format!(r#"{{"LongType_{namespace}": "http://example.test/types/k00s4n46l67paax1ejooc102cv85hxbjfhb3k9vz5jlc4pemneh949ymvj80sx26yu5yrbfk5a5e97ou9hlkwvwtpgkel1xkc7p0kaw56yppk9e204dsdqsc0pkzevjj4dqi74bnxjx154ti8riayibqup0qziijgzglb2jehsit8uz5okllym9c6day3ezloqucpyjts695g8d6u7ts79meumweu0gee26b2476yzeobd1cprz13pzyu5ofc4l5xn2qp45yty7akumtzjf9lf7ty3gk1nvxmqfbu6qj6mprykz3z13rlyrgmncwxbe6cz22pa8jgtm3xaxki17dtm0z9po3bef1mjd57854vjrjwstaeel6gmxrwdkc5rzkcgow8q0mm71fuon6cw8lbx54k2brdn8htyn3xawlqbykl860no69s2xfpzlw8xib1bngn87x6shkp135mnql1yo54qc83ncv2tiokyjzcas3m5o94dbxi5v3wy2eqgxq3hmvetpijqgsn6ni55ncv7xamjsdq0dh6bo25689x1bxjwz236o2ob3ofw5jkaruvtnd3lmjakjo9hk8hzhhnn8bfpogpn17aztuw0c5cz4lcfq4q7ksvx5qysdhsf5uijttyhr2odx16g4n9xmc5tv1lb16mkcnnkvao2pw7ymrlx4jvxjsfdptu9ykzbrrnohbnym56nk1ysi7ydiafk3h4deuhl6s2s0z9hrwa0q3uqwi7wwnkfrkavckwn6bcblu9oiukmglt0qunzyscptuot1mf33ll2dy5f95k9habt7jae2f0xz25qltotejk0u9qwz8hx86dfmp3v4s6p94rmuu480oqnl87kglq418rp6qfhsen8ka696p10zql1masy0mqz5sodxqxv716dqkolgvoklu7fcd4lkzyt3ckjly2kzyvhn6lghvchyv9z4rfzd7qjw75fug3z024sd0pdqp4qr91o1yvmuxss2bfreh54u1vyo7qeil8emxz796z5g7jfz5m0oornbkmdvzmhzsfrcp5y1wdx4rbb8ic59yu3frdqutlcikcq6ky7obfq1f7wy4"}}"#).parse::<InlineContext>()?)]))?.with_context_delivery(ContextDelivery::LdJsonBody);
    let building_long_expanded_type_id = namespace.urn("Building", "long-expanded-type")?;
    let long_type_type = namespace.entity_type("LongType")?;
    let building_long_expanded_type = Entity::builder()
        .id(building_long_expanded_type_id.clone())
        .entity_type(long_type_type)
        .build()?;
    let answer = body_context.with_responses().entities().create(&building_long_expanded_type).send().await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_long_expanded_type_id)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
