//! A registration id pattern past the bound is refused.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
    query::IdPattern,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Create Context Source Registration whose `EntityInfo` `idPattern` is over 1,100 octets answers 400
/// `BadRequestData`, and the registration does not exist
/// Clause: 5.9.2, 4.4, 5.5.4
/// Recorded: ETSI GS CIM 009 sets no length on an identifier; Andromeda bounds every identifier it stores at 1,024
/// octets, the value as stored, which for a term is its expansion
#[gtest]
#[tokio::test]
async fn a_lim_idn_9007_a_registration_id_pattern_past_the_bound_is_refused() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let registration_long_id_pattern_id = namespace.registration_id("long-id-pattern")?;
    let building_type = namespace.entity_type("Building")?;
    let registration_long_id_pattern = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_long_id_pattern_id.clone())
        .information([RegistrationInfo::builder().entities([EntityInfo::new(building_type).with_id_pattern(format!("^urn:ngsi-ld:Building:{namespace}-9ewsf6wz0bcg2owf5v80m2ko107m81tr6o047jtt4zgxiqjve9zk013sjunrabz09i1jsadctha7dc6ajl2xoebmq5bimhkcyomn8pvoahquo0du0sks0h1dy05buyijkw4z6fum4c2oo2l6vklc3d93tdgtouohmkmgquxajxmo1oqq8vuz3tnhkeqegonicrxuesgv0xhxsh8wi7l6b2oun1nnz2y86dmp3wu6s79nq6oonppn4tcg1jarvztepfchtlkyuj83khxjbc6zk0af6z1mumgqrbfvrfaq988i2nvxpnunrpnaylxq6hqbp25lhwohm6sbt07886c1foujibw2pacn8q7joe8hqtpbfvumvewp5pmjykbsrx25rruwfv7jboo5bkxpba0xbo7kld0v0tcitw54ryx2v8yce9dr9s36rbudxl2pb3vqt5vj8pbw7cteckn3pjciwnpawh0tcdu5dd8uu0chpjmt3i3y7sae6zkw9wm8xj88b6antd88doru5yxe8qbc191uvcw0rypc48rmyd5v9jv6wh2lrsugw0sdzaejxaj2gott37k5r9f6fuorxkx1mvigsqtzor3ad5ijpy6dqbbmfasmaa39hqm3pclnyrexgy04p3kwk3nnmyc3qp7vtrs1z1qu5nqzj0joi115lw81rkgaswnodzoeopm43x0vcwgu0kbucvuxgdv737pzj193l1fy6cp2ljp09739qptrf3u92dmflv1a8blszzfaaoi2inbi2zecvcegkkjc89t475ufuul8art43wx6rg44gh9tusmzrdhrafk18dptkqk2e5y1kl2b5w92fp8296bu3nd1rdy4bzi17h3eoqburm0taclsip0q8x5e697ac783w2twa49n8ehesqornbx6a5j8i6pilckxo5ohpah9t1y1p17jehokp4gi3jq0odz69ef7o9qkwjaska8mlqjy0j8smxq0y07z98utjt2xvwd55jwvsiukshqvdge3ix6jr79m62dtqti52ky7jqy0c2bvx5yb2g9vtyec6rt0e3b35n6juncg31fjvj5mmefxb5vjcqnb").parse::<IdPattern>()?)]).build()?])
        .build()?;
    let answer = broker.with_responses().registrations().register(&registration_long_id_pattern).send().await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&registration_long_id_pattern_id)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
