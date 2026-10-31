//! A sub attribute chain at the write limit is matched by q and ordered by orderby.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Done, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType, Property},
    query::{Locality, ordering::OrderBy, q},
};

/// Pins: a sub-Attribute chain of 32 levels, the depth a write is admitted to, is matched by q on its deepest
/// Property and ordered by `orderBy`, and a q path one level deeper answers 400 `BadRequestData`
/// Clause: 4.9, 4.23.3, 4.5.2
#[gtest]
#[tokio::test]
async fn a_lim_dep_9015_a_sub_attribute_chain_at_the_write_limit_is_matched_by_q_and_ordered_by_orderby() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let chain_depth_a_id = namespace.urn("ChainDepth", "a")?;
    let chain_depth_type = namespace.entity_type("ChainDepth")?;
    let chain_depth_b_id = namespace.urn("ChainDepth", "b")?;
    let s31_attribute = Property::builder(132).property("s32", "alpha");
    let s30_attribute = Property::builder(131).property("s31", s31_attribute);
    let s29_attribute = Property::builder(130).property("s30", s30_attribute);
    let s28_attribute = Property::builder(129).property("s29", s29_attribute);
    let s27_attribute = Property::builder(128).property("s28", s28_attribute);
    let s26_attribute = Property::builder(127).property("s27", s27_attribute);
    let s25_attribute = Property::builder(126).property("s26", s26_attribute);
    let s24_attribute = Property::builder(125).property("s25", s25_attribute);
    let s23_attribute = Property::builder(124).property("s24", s24_attribute);
    let s22_attribute = Property::builder(123).property("s23", s23_attribute);
    let s21_attribute = Property::builder(122).property("s22", s22_attribute);
    let s20_attribute = Property::builder(121).property("s21", s21_attribute);
    let s19_attribute = Property::builder(120).property("s20", s20_attribute);
    let s18_attribute = Property::builder(119).property("s19", s19_attribute);
    let s17_attribute = Property::builder(118).property("s18", s18_attribute);
    let s16_attribute = Property::builder(117).property("s17", s17_attribute);
    let s15_attribute = Property::builder(116).property("s16", s16_attribute);
    let s14_attribute = Property::builder(115).property("s15", s15_attribute);
    let s13_attribute = Property::builder(114).property("s14", s14_attribute);
    let s12_attribute = Property::builder(113).property("s13", s13_attribute);
    let s11_attribute = Property::builder(112).property("s12", s12_attribute);
    let s10_attribute = Property::builder(111).property("s11", s11_attribute);
    let s9_attribute = Property::builder(110).property("s10", s10_attribute);
    let s8_attribute = Property::builder(109).property("s9", s9_attribute);
    let s7_attribute = Property::builder(108).property("s8", s8_attribute);
    let s6_attribute = Property::builder(107).property("s7", s7_attribute);
    let s5_attribute = Property::builder(106).property("s6", s6_attribute);
    let s4_attribute = Property::builder(105).property("s5", s5_attribute);
    let s3_attribute = Property::builder(104).property("s4", s4_attribute);
    let s2_attribute = Property::builder(103).property("s3", s3_attribute);
    let s1_attribute = Property::builder(102).property("s2", s2_attribute);
    let s31_attribute_2 = Property::builder(232).property("s32", "beta");
    let s30_attribute_2 = Property::builder(231).property("s31", s31_attribute_2);
    let s29_attribute_2 = Property::builder(230).property("s30", s30_attribute_2);
    let s28_attribute_2 = Property::builder(229).property("s29", s29_attribute_2);
    let s27_attribute_2 = Property::builder(228).property("s28", s28_attribute_2);
    let s26_attribute_2 = Property::builder(227).property("s27", s27_attribute_2);
    let s25_attribute_2 = Property::builder(226).property("s26", s26_attribute_2);
    let s24_attribute_2 = Property::builder(225).property("s25", s25_attribute_2);
    let s23_attribute_2 = Property::builder(224).property("s24", s24_attribute_2);
    let s22_attribute_2 = Property::builder(223).property("s23", s23_attribute_2);
    let s21_attribute_2 = Property::builder(222).property("s22", s22_attribute_2);
    let s20_attribute_2 = Property::builder(221).property("s21", s21_attribute_2);
    let s19_attribute_2 = Property::builder(220).property("s20", s20_attribute_2);
    let s18_attribute_2 = Property::builder(219).property("s19", s19_attribute_2);
    let s17_attribute_2 = Property::builder(218).property("s18", s18_attribute_2);
    let s16_attribute_2 = Property::builder(217).property("s17", s17_attribute_2);
    let s15_attribute_2 = Property::builder(216).property("s16", s16_attribute_2);
    let s14_attribute_2 = Property::builder(215).property("s15", s15_attribute_2);
    let s13_attribute_2 = Property::builder(214).property("s14", s14_attribute_2);
    let s12_attribute_2 = Property::builder(213).property("s13", s13_attribute_2);
    let s11_attribute_2 = Property::builder(212).property("s12", s12_attribute_2);
    let s10_attribute_2 = Property::builder(211).property("s11", s11_attribute_2);
    let s9_attribute_2 = Property::builder(210).property("s10", s10_attribute_2);
    let s8_attribute_2 = Property::builder(209).property("s9", s9_attribute_2);
    let s7_attribute_2 = Property::builder(208).property("s8", s8_attribute_2);
    let s6_attribute_2 = Property::builder(207).property("s7", s7_attribute_2);
    let s5_attribute_2 = Property::builder(206).property("s6", s6_attribute_2);
    let s4_attribute_2 = Property::builder(205).property("s5", s5_attribute_2);
    let s3_attribute_2 = Property::builder(204).property("s4", s4_attribute_2);
    let s2_attribute_2 = Property::builder(203).property("s3", s3_attribute_2);
    let s1_attribute_2 = Property::builder(202).property("s2", s2_attribute_2);
    let entities = [
        Entity::builder()
            .id(chain_depth_a_id.clone())
            .entity_type(chain_depth_type.clone())
            .property("p", Property::builder(101).property("s1", s1_attribute))
            .build()?,
        Entity::builder()
            .id(chain_depth_b_id.clone())
            .entity_type(chain_depth_type.clone())
            .property("p", Property::builder(201).property("s1", s1_attribute_2))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&chain_depth_type)
        .locality(Locality::Local)
        .q(q::attr("p")
            .sub_attr("s1")
            .sub_attr("s2")
            .sub_attr("s3")
            .sub_attr("s4")
            .sub_attr("s5")
            .sub_attr("s6")
            .sub_attr("s7")
            .sub_attr("s8")
            .sub_attr("s9")
            .sub_attr("s10")
            .sub_attr("s11")
            .sub_attr("s12")
            .sub_attr("s13")
            .sub_attr("s14")
            .sub_attr("s15")
            .sub_attr("s16")
            .sub_attr("s17")
            .sub_attr("s18")
            .sub_attr("s19")
            .sub_attr("s20")
            .sub_attr("s21")
            .sub_attr("s22")
            .sub_attr("s23")
            .sub_attr("s24")
            .sub_attr("s25")
            .sub_attr("s26")
            .sub_attr("s27")
            .sub_attr("s28")
            .sub_attr("s29")
            .sub_attr("s30")
            .sub_attr("s31")
            .sub_attr("s32")
            .eq("alpha"))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&chain_depth_a_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&chain_depth_type)
        .locality(Locality::Local)
        .ordering(
            "p.s1.s2.s3.s4.s5.s6.s7.s8.s9.s10.s11.s12.s13.s14.s15.s16.s17.s18.s19.s20.s21.s22.s23.s24.s25.s26.s27.s28.s29.s30.s31.s32".parse::<OrderBy>()?,
        )
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(page_2.items(), nth(0, has_id(&chain_depth_a_id)));
    expect_that!(page_2.items(), nth(1, has_id(&chain_depth_b_id)));
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&chain_depth_type)
        .locality(Locality::Local)
        .q(q::attr("p")
            .sub_attr("s1")
            .sub_attr("s2")
            .sub_attr("s3")
            .sub_attr("s4")
            .sub_attr("s5")
            .sub_attr("s6")
            .sub_attr("s7")
            .sub_attr("s8")
            .sub_attr("s9")
            .sub_attr("s10")
            .sub_attr("s11")
            .sub_attr("s12")
            .sub_attr("s13")
            .sub_attr("s14")
            .sub_attr("s15")
            .sub_attr("s16")
            .sub_attr("s17")
            .sub_attr("s18")
            .sub_attr("s19")
            .sub_attr("s20")
            .sub_attr("s21")
            .sub_attr("s22")
            .sub_attr("s23")
            .sub_attr("s24")
            .sub_attr("s25")
            .sub_attr("s26")
            .sub_attr("s27")
            .sub_attr("s28")
            .sub_attr("s29")
            .sub_attr("s30")
            .sub_attr("s31")
            .sub_attr("s32")
            .sub_attr("s33")
            .eq("alpha"))
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
