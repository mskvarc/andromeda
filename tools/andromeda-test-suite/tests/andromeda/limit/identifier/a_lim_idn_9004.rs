//! An instance id past the bound is refused and creates nothing.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, InstanceId, NgsiLdMediaType, Property, evolution::TemporalEntity},
};

/// Pins: Create or Update Temporal Evolution whose one instance carries an `instanceId` of over 1,100 octets
/// answers 400 `BadRequestData`, and no Temporal Evolution of the Entity exists
/// Clause: 4.5.7, 5.6.11, 5.5.4
/// Recorded: ETSI GS CIM 009 sets no length on an identifier; Andromeda bounds every identifier it stores at 1,024
/// octets, the value as stored, which for a term is its expansion
#[gtest]
#[tokio::test]
async fn a_lim_idn_9004_an_instance_id_past_the_bound_is_refused_and_creates_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_long_instance_id_id = namespace.urn("Building", "long-instance-id")?;
    let building_type = namespace.entity_type("Building")?;
    let building_long_instance_id_evolution = TemporalEntity::builder()
        .id(building_long_instance_id_id.clone())
        .entity_type(building_type)
        .identified("temperature", format!("urn:ngsi-ld:Instance:{namespace}-sfbt1y7wimr2ao3bgf50b61wdncxlnxxsuf4l42kll8u7mhfg1crrvgxs8t39nqjgh5pv8jgyk2cd0zbkoaod82fwby5mceq39m3snfg97k89pqg0ka2v0k0ep8s10t99dsgbiv91pkfv98hp942ravybjznbl21f0r489l78tqfod5yb30su0wtrvkyc049q8rfynqj6xh3hr2acqhp4c0frmxjfl6zxsfkchlbiis4sdcgubiwfknn1uo81x8o9t3iwytc5zwn8knu01dcu14k0xpucs2lzx5b81l2m1e3azyqdc0ywrkje638he14jthb7n8mwtrs1588svbo9jeihtqhffc4c6qr80ameiln5djqayc0cbijaw690a5f3uscknthjkih16mk78nergih61wen6yvgpjx13nk99pvcgije9xcc2dkqwn3vtf14gz8yyblebcvjgvzj0ti7r786zlrzvjavt9f54wd0frlhbox1pf7qqmnl8j5u0vx8o1dyuhedihlh1siwkgxvs3nzs6ckc38zci4ax2o8jmcrmht5arq574l10mhnya0hrlronpas6m0fl0mmc8nebalxcb60u4fordiagdmgfpif27r556cc9kau64azpucv796l5xfnsutso0pcdeuzy0of6kom1h8f2d4reyg7ocvl8yd7qknvji8kx4kzbcm81onwj3b94mo8282eg3w90vfouktvmbw3spmfkcdxfoe9fmy7hvouhpxvip3jtrgu5pl18r6mlzz0tlza4hffatlb1i4vytwyqwnzj0uyoih0jwby07ekb3rfpbjtxho4rh5p053am3x5efo0jew9chrb6hnwm48uvioxaotya26okbh7728i6z1s2evodd8why63nmjol2gs6qq5dga2q5xxowcm9z2ea10cf863uljt4fabutc54wwb3aso8vedeopp8qzr33cdogjvtvpeuu2bizwpi0qlei0etp16rcvu70vosbnrd6o5kc63hjkbms02x640y5utw9zedqhwrxauj842rur7t9kf1lks4jkuw661zn8w96lcigcr2nk232a7zl8dkph").parse::<InstanceId>()?, Property::builder(1).observed_at("2026-01-01T00:00:00Z".parse()?))
        .build()?;
    let answer = broker
        .with_responses()
        .temporal()
        .create_or_update(&building_long_instance_id_evolution)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&building_long_instance_id_id)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    Ok(())
}
