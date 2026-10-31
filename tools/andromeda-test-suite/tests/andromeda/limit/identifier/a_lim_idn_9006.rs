//! Path query subscription and registration ids past the bound are bad request data.

use andromeda_test_suite::{broker, config::SuiteConfig, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{ErrorType, NgsiLdMediaType},
    registration::{CSourceRegistration, RegistrationInfo},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: Retrieve Entity and Query Entities by an id of over 1,100 octets, and Create Subscription and Create
/// Context Source Registration with an id of over 1,100 octets, each answer 400 `BadRequestData`
/// Clause: 5.7.1, 5.7.2, 5.8.1, 5.9.2, 5.5.4
/// Recorded: ETSI GS CIM 009 sets no length on an identifier; Andromeda bounds every identifier it stores at 1,024
/// octets, the value as stored, which for a term is its expansion
#[gtest]
#[tokio::test]
async fn a_lim_idn_9006_path_query_subscription_and_registration_ids_past_the_bound_are_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let building_long_path_mn18di4c6k1p0mbojin4yxpbbs_id = namespace.urn("Building", "long-path:mn18di4c6k1p0mbojin4yxpbbs88h6ca39uu11mdqmy17jdgei89cd0nxf8ug0c03ecwgpfirw1w74yccja7h4clcb1ds1keskgecb18br5u9zbghn2ptoj6yzuf6ejcbjzpuemru14n5pa7ggbnngj9ic8kk7ddy58h0nvkod9atq0m725uqrs4dqzddtlsooq60bun3sga6gadeiyfwh4z5m0qqg5ex3orjgt9owi5d3g24to224nkfreomcgrp3hlw0216mrngi9zt5e276ctjx56xwbv0baxkwxpi6d6w09fjao745kj8gyzqdt0s5znd2rkpjjxbw85gkxwvy5rs1u05st8ryr45wsdwzqlpn4mefxjm4vd5qp6t1k8cymc8dvvmxy0sdffuwe36arb8qhq3xt2t2vtqktr5awgexdfcyp5d0e0o68o5s4gskfbgbsjjvtbnld7t29om7m9sle6hh0ad1wlqsazsj1w8awdu412tinhq4xpaiid6ex177iijrtfjv6zmvmlviy466xezr6edays6m87cug2soudclsm5wbvyr5h2t7yotv0of7ai5x4psergmbndwy299d0447v4nqs2ii38il48dmoghio5085w61p87jxdkk83d83lz0ixnzusrcthz93om2c1nj6tipey24zwgf7yjua5yksjwqaez1qsq2v3zuf18y8wn3t3uq51mzw7f3azz2vsxvcylz4onnqozcwlms21b3rua63jca7yd35f6h2m0fk2xymxeh09qd3zjc9iqueowc9gbcj8zm7nfnn4b6bszelbjpjrixmakux5kit9x8tsecikty4ohqyq372iy4oj5pc81ia8y9v9k7ld94lbnzwbbjy2xsenyb54h3vkrl4hk10i5m36z4x661o2487p7jqfp75a9jrs293xln52gllufkzfiyuxryhej09ki9i8s5z840wsn6nr6sd9kymcl7fyuewu7crwo3j3oo7jxw185jnq0e2ts3mhub5pjlf3qrz1xeoxowhdivja2af1s8rw3q384hggzwh1d81t001rjj89nsfpb9tqogqpc1dhlqb")?;
    let subscription_long_id_4rsmeeq8vqxzlpp5ef10d2wa_id = namespace.subscription_id("long-id:4rsmeeq8vqxzlpp5ef10d2wa4qivo7jgztc7fk3jq8dla2jw0et23waxkql4w7d7pkn59fvvdzvhtq9ft33s6hbsn498hadxmsgc9103alwamoymvyewclefsoi34goxrb5kt83n1tn6jvdoh4fp1utcigzfxnk8bl2d72dor8mg982xv6nzmkawp7muy4vepedid58f2git5v78yve5wx4t5mo79powgtskz5r0qblh738cd2pqghjpt91qr8cld3hfp8vjhbob8q5va1f8bxsghljlb9s5cxh5jv3vrmvqm0638ie11zyv11kehi1mjs56yvlrvfge5k0b79aalq65bt4h5908zyuu9paqa2b1k6ydo809c69rmii65ma5925mfykn4rpt370d5y2ngtr7jdhhx30994zo0jte3q15c6c95a5fqlkr58825y18g3p4b0ncy8uf6qs5xp0gwnnyke4dodv5dd2jh51chp1wy7429a7zhsya3smouh95gveh033mkie9oaior77jmvfgeqjegll6ntdvekgb8locsbhdusyyj4x0zrzypzda2qpilh5lewva2re72n662p7tp2pp667io3k6ih1p1kx57jaab9tek5yms7mttqhzun33gkyp3a8ck4z96zbac8j1nuoonklgoecv49h439spjjc70091n9pn8i5oexnt1en4fy71uh561k8ymnpuqpw5kdw9l7v0h29dj2ts00af8swhodxisa344eck0i993ne9o6ccbo1p7qaoh1ahroq26o5jpk4fw58av8apynxqdkxintfohegniualost5kihgzu39aofvx68ijca5pee8gcf7hrtzllyykx9d51kqx2i362np24m0ku0wrnzt5e3tf4bevwxovpwde92csjae8ainp0txdm2i9wqau0ked17z83y3mk2j2oe6l15irf3j3sg2xvle3d20kkviy6lli7phwa36szt7gnidj8261nihdkxwwfmy4ls2j3gdwdp3xnjqlf36u020vuuymyr5p1u4aqj0o9fljzjpapki4n9b1uoutmhrhdjz6fot7kyh0rqx821c")?;
    let building_type = namespace.entity_type("Building")?;
    let registration_long_id_hi8loi30nnjzkimmb0peg0ui_id = namespace.registration_id("long-id:hi8loi30nnjzkimmb0peg0uil67ro2imasqf78rh0kmxec15xkyqy6loltm80bomduwzg5j57n821k0p3s3opca109q2i76a3xxfozm9gihs62ejmk7f26u64zzzevp7v7x7fvgzsz3ifzpdit4mbj4b3bpeh0cdr4vuzwncwmbv6hhxeemk8q2z57lowyqnycr56oxusrx6j1uc9uouykv8kx5ayac7u7h5tqq0rhyf4ke2sv7p3l3qye614y8rngg02mmkrqiodto6kp5y1upesx0aqygzrldpt4odpbjinuzqgh8ohmcv0adsq7q5zrw95irfvqbdgej08ka2tmay9wnvy8twnguu8c8bad969lz79fbq2yz98ohfygwiyvcegdgjsh6pz51gvjqjnr0jamqhv8rdssgfhm7ia4bwua7ni0kh4uvhc39fkbewb7cxtgfag4ly7kzisaeaewo6jhd00r8rpmbv1svqjldsexjool2howairts6mxat516bwtmxejexudqu5fbduvkp5njcgthk5hpd2252ymiexw741y4o6ljwlvsk2yndns93noqt8hyuti6z37vlgwnr5cqzfip25lvas36bv02llum23r0k9wx7n82f5mq9u333fi97ssh4q2lvpd5r8hrj3app51lq4vd3i7qajjdnc5ty1f1k539igrzagwxvcup69jckjcwclviksjpo4pm0h4fj8i7xwqkgbk4gamrw824yfs1nqbzoaq3oqka5soduqg2lo5dazdtrzwi3o2qjk5of8j269v9xlyl7stv6p8u2nd6wkwir2h4ohb8d2rb6y1qv0h0uk17hr3219v9unoq43i6nwkw4pw6c98n94f1pzfkmlpydib1yh1pgjzfl5frd44k3gbm1lgvv8ox4j5hkgqwhu63n6m138blhl59xivtk8gzebhnp6yqshq1ub712yj5x4bxonixxo76xro56z2ycqgw089avq3oz30j4v1xuwhj9sm2usrqud7le1mrkdtzvz38s7p7wvqe9mvyc8p5sj1qnrewiasjw2b0cqb66eera9uyubxvwe862uielxxg5")?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_long_path_mn18di4c6k1p0mbojin4yxpbbs_id)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let answer_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .ids(vec![building_long_path_mn18di4c6k1p0mbojin4yxpbbs_id.clone()])
        .entity_type(&namespace.entity_type("Building")?)
        .send()
        .await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let subscription_long_id_4rsmeeq8vqxzlpp5ef10d2wa = Subscription::builder()
        .id(subscription_long_id_4rsmeeq8vqxzlpp5ef10d2wa_id)
        .entities([building_type.as_str()])
        .notification(NotificationParams::builder(Endpoint::builder(config.unreachable_origin().as_str()).build()?).build()?)
        .build()?;
    let answer_3 = broker
        .with_responses()
        .subscriptions()
        .create(&subscription_long_id_4rsmeeq8vqxzlpp5ef10d2wa)
        .send()
        .await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let registration_long_id_hi8loi30nnjzkimmb0peg0ui = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_long_id_hi8loi30nnjzkimmb0peg0ui_id)
        .information([RegistrationInfo::builder().entities([building_type.as_str()]).build()?])
        .build()?;
    let answer_4 = broker
        .with_responses()
        .registrations()
        .register(&registration_long_id_hi8loi30nnjzkimmb0peg0ui)
        .send()
        .await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
