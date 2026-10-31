//! The longest sub attribute chain the reader admits is refused by the limit.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::{answered, problem},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, NgsiLdMediaType, Property},
};

/// Pins: Create Entity with p carrying a chain of 124 sub-Attributes, the longest the request reader admits,
/// answers 400 `BadRequestData` naming p and the 32-level limit, and stores nothing
/// Clause: 5.6.1.4, 5.5.4
#[gtest]
#[tokio::test]
async fn a_lim_dep_9013_the_longest_sub_attribute_chain_the_reader_admits_is_refused_by_the_limit() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let nesting_probe_id = namespace.urn("NestingProbe", 1)?;
    let nesting_probe_type = namespace.entity_type("NestingProbe")?;
    let s_attribute = Property::builder(1).property("s", 1);
    let s_attribute_2 = Property::builder(1).property("s", s_attribute);
    let s_attribute_3 = Property::builder(1).property("s", s_attribute_2);
    let s_attribute_4 = Property::builder(1).property("s", s_attribute_3);
    let s_attribute_5 = Property::builder(1).property("s", s_attribute_4);
    let s_attribute_6 = Property::builder(1).property("s", s_attribute_5);
    let s_attribute_7 = Property::builder(1).property("s", s_attribute_6);
    let s_attribute_8 = Property::builder(1).property("s", s_attribute_7);
    let s_attribute_9 = Property::builder(1).property("s", s_attribute_8);
    let s_attribute_10 = Property::builder(1).property("s", s_attribute_9);
    let s_attribute_11 = Property::builder(1).property("s", s_attribute_10);
    let s_attribute_12 = Property::builder(1).property("s", s_attribute_11);
    let s_attribute_13 = Property::builder(1).property("s", s_attribute_12);
    let s_attribute_14 = Property::builder(1).property("s", s_attribute_13);
    let s_attribute_15 = Property::builder(1).property("s", s_attribute_14);
    let s_attribute_16 = Property::builder(1).property("s", s_attribute_15);
    let s_attribute_17 = Property::builder(1).property("s", s_attribute_16);
    let s_attribute_18 = Property::builder(1).property("s", s_attribute_17);
    let s_attribute_19 = Property::builder(1).property("s", s_attribute_18);
    let s_attribute_20 = Property::builder(1).property("s", s_attribute_19);
    let s_attribute_21 = Property::builder(1).property("s", s_attribute_20);
    let s_attribute_22 = Property::builder(1).property("s", s_attribute_21);
    let s_attribute_23 = Property::builder(1).property("s", s_attribute_22);
    let s_attribute_24 = Property::builder(1).property("s", s_attribute_23);
    let s_attribute_25 = Property::builder(1).property("s", s_attribute_24);
    let s_attribute_26 = Property::builder(1).property("s", s_attribute_25);
    let s_attribute_27 = Property::builder(1).property("s", s_attribute_26);
    let s_attribute_28 = Property::builder(1).property("s", s_attribute_27);
    let s_attribute_29 = Property::builder(1).property("s", s_attribute_28);
    let s_attribute_30 = Property::builder(1).property("s", s_attribute_29);
    let s_attribute_31 = Property::builder(1).property("s", s_attribute_30);
    let s_attribute_32 = Property::builder(1).property("s", s_attribute_31);
    let s_attribute_33 = Property::builder(1).property("s", s_attribute_32);
    let s_attribute_34 = Property::builder(1).property("s", s_attribute_33);
    let s_attribute_35 = Property::builder(1).property("s", s_attribute_34);
    let s_attribute_36 = Property::builder(1).property("s", s_attribute_35);
    let s_attribute_37 = Property::builder(1).property("s", s_attribute_36);
    let s_attribute_38 = Property::builder(1).property("s", s_attribute_37);
    let s_attribute_39 = Property::builder(1).property("s", s_attribute_38);
    let s_attribute_40 = Property::builder(1).property("s", s_attribute_39);
    let s_attribute_41 = Property::builder(1).property("s", s_attribute_40);
    let s_attribute_42 = Property::builder(1).property("s", s_attribute_41);
    let s_attribute_43 = Property::builder(1).property("s", s_attribute_42);
    let s_attribute_44 = Property::builder(1).property("s", s_attribute_43);
    let s_attribute_45 = Property::builder(1).property("s", s_attribute_44);
    let s_attribute_46 = Property::builder(1).property("s", s_attribute_45);
    let s_attribute_47 = Property::builder(1).property("s", s_attribute_46);
    let s_attribute_48 = Property::builder(1).property("s", s_attribute_47);
    let s_attribute_49 = Property::builder(1).property("s", s_attribute_48);
    let s_attribute_50 = Property::builder(1).property("s", s_attribute_49);
    let s_attribute_51 = Property::builder(1).property("s", s_attribute_50);
    let s_attribute_52 = Property::builder(1).property("s", s_attribute_51);
    let s_attribute_53 = Property::builder(1).property("s", s_attribute_52);
    let s_attribute_54 = Property::builder(1).property("s", s_attribute_53);
    let s_attribute_55 = Property::builder(1).property("s", s_attribute_54);
    let s_attribute_56 = Property::builder(1).property("s", s_attribute_55);
    let s_attribute_57 = Property::builder(1).property("s", s_attribute_56);
    let s_attribute_58 = Property::builder(1).property("s", s_attribute_57);
    let s_attribute_59 = Property::builder(1).property("s", s_attribute_58);
    let s_attribute_60 = Property::builder(1).property("s", s_attribute_59);
    let s_attribute_61 = Property::builder(1).property("s", s_attribute_60);
    let s_attribute_62 = Property::builder(1).property("s", s_attribute_61);
    let s_attribute_63 = Property::builder(1).property("s", s_attribute_62);
    let s_attribute_64 = Property::builder(1).property("s", s_attribute_63);
    let s_attribute_65 = Property::builder(1).property("s", s_attribute_64);
    let s_attribute_66 = Property::builder(1).property("s", s_attribute_65);
    let s_attribute_67 = Property::builder(1).property("s", s_attribute_66);
    let s_attribute_68 = Property::builder(1).property("s", s_attribute_67);
    let s_attribute_69 = Property::builder(1).property("s", s_attribute_68);
    let s_attribute_70 = Property::builder(1).property("s", s_attribute_69);
    let s_attribute_71 = Property::builder(1).property("s", s_attribute_70);
    let s_attribute_72 = Property::builder(1).property("s", s_attribute_71);
    let s_attribute_73 = Property::builder(1).property("s", s_attribute_72);
    let s_attribute_74 = Property::builder(1).property("s", s_attribute_73);
    let s_attribute_75 = Property::builder(1).property("s", s_attribute_74);
    let s_attribute_76 = Property::builder(1).property("s", s_attribute_75);
    let s_attribute_77 = Property::builder(1).property("s", s_attribute_76);
    let s_attribute_78 = Property::builder(1).property("s", s_attribute_77);
    let s_attribute_79 = Property::builder(1).property("s", s_attribute_78);
    let s_attribute_80 = Property::builder(1).property("s", s_attribute_79);
    let s_attribute_81 = Property::builder(1).property("s", s_attribute_80);
    let s_attribute_82 = Property::builder(1).property("s", s_attribute_81);
    let s_attribute_83 = Property::builder(1).property("s", s_attribute_82);
    let s_attribute_84 = Property::builder(1).property("s", s_attribute_83);
    let s_attribute_85 = Property::builder(1).property("s", s_attribute_84);
    let s_attribute_86 = Property::builder(1).property("s", s_attribute_85);
    let s_attribute_87 = Property::builder(1).property("s", s_attribute_86);
    let s_attribute_88 = Property::builder(1).property("s", s_attribute_87);
    let s_attribute_89 = Property::builder(1).property("s", s_attribute_88);
    let s_attribute_90 = Property::builder(1).property("s", s_attribute_89);
    let s_attribute_91 = Property::builder(1).property("s", s_attribute_90);
    let s_attribute_92 = Property::builder(1).property("s", s_attribute_91);
    let s_attribute_93 = Property::builder(1).property("s", s_attribute_92);
    let s_attribute_94 = Property::builder(1).property("s", s_attribute_93);
    let s_attribute_95 = Property::builder(1).property("s", s_attribute_94);
    let s_attribute_96 = Property::builder(1).property("s", s_attribute_95);
    let s_attribute_97 = Property::builder(1).property("s", s_attribute_96);
    let s_attribute_98 = Property::builder(1).property("s", s_attribute_97);
    let s_attribute_99 = Property::builder(1).property("s", s_attribute_98);
    let s_attribute_100 = Property::builder(1).property("s", s_attribute_99);
    let s_attribute_101 = Property::builder(1).property("s", s_attribute_100);
    let s_attribute_102 = Property::builder(1).property("s", s_attribute_101);
    let s_attribute_103 = Property::builder(1).property("s", s_attribute_102);
    let s_attribute_104 = Property::builder(1).property("s", s_attribute_103);
    let s_attribute_105 = Property::builder(1).property("s", s_attribute_104);
    let s_attribute_106 = Property::builder(1).property("s", s_attribute_105);
    let s_attribute_107 = Property::builder(1).property("s", s_attribute_106);
    let s_attribute_108 = Property::builder(1).property("s", s_attribute_107);
    let s_attribute_109 = Property::builder(1).property("s", s_attribute_108);
    let s_attribute_110 = Property::builder(1).property("s", s_attribute_109);
    let s_attribute_111 = Property::builder(1).property("s", s_attribute_110);
    let s_attribute_112 = Property::builder(1).property("s", s_attribute_111);
    let s_attribute_113 = Property::builder(1).property("s", s_attribute_112);
    let s_attribute_114 = Property::builder(1).property("s", s_attribute_113);
    let s_attribute_115 = Property::builder(1).property("s", s_attribute_114);
    let s_attribute_116 = Property::builder(1).property("s", s_attribute_115);
    let s_attribute_117 = Property::builder(1).property("s", s_attribute_116);
    let s_attribute_118 = Property::builder(1).property("s", s_attribute_117);
    let s_attribute_119 = Property::builder(1).property("s", s_attribute_118);
    let s_attribute_120 = Property::builder(1).property("s", s_attribute_119);
    let s_attribute_121 = Property::builder(1).property("s", s_attribute_120);
    let s_attribute_122 = Property::builder(1).property("s", s_attribute_121);
    let s_attribute_123 = Property::builder(1).property("s", s_attribute_122);
    let nesting_probe = Entity::builder()
        .id(nesting_probe_id.clone())
        .entity_type(nesting_probe_type)
        .property("p", Property::builder(1).property("s", s_attribute_123))
        .build()?;
    let answer = broker.with_responses().entities().create(&nesting_probe).send().await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_detail_containing("'https://uri.etsi.org/ngsi-ld/default-context/p'")
            .with_detail_containing("32 levels")
    );
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&nesting_probe_id)
        .send()
        .await;
    expect_that!(&answer_2, answered(StatusCode::NOT_FOUND));
    Ok(())
}
