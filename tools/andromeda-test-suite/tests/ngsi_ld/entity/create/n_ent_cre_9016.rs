//! A list relationship member of six thousand octets round trips.

use andromeda_test_suite::{
    attribute_matcher::attribute_of,
    broker,
    entity_matcher::has_attribute,
    identity_matcher::has_id,
    namespace::Namespace,
    problem::answered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateEntity},
    model::{AttributeKind, Entity, EntityId, Lossless, NgsiLdMediaType, RawJson},
    query::{Locality, q},
};

/// The second member of the `servedBy` list: a URN of more than six thousand octets.
const LONG_TAIL: &str = "ptgz4jfebz9sdo78xrlgqnbqrmktsxfvy6plp4rf9tast6m01s12kotqcfc3r784vjme0m2rlw1u9mugdorphvls3bcwfsubusuj0esm2siqykvaxc3kxxsg2n1nhdddkjc85puch7s0m4mp205co02p1n5mccqqp7no0qjuduhzc5yf1nkvs4u07nrvz5ermczirdk34zynankaqhzyo9dmkv9472bfch59qicxf7atwef83yntyo5zgehx6102emt41h9kxklju5vq8akath8h547e7p0swoladu83t622zjqxvi1fjlsxmwfezlvxultbb7fwgkl5ehl4otzp5otxou872z6zus20aql839xzybj6e3wtfq4o4ejpetidkz98qb7pkgncufhrdslj0if9xa9j60jnt46eykkq6z8tzvkyd1brbtjfkhaoo9a48l2yvl7mg4wt0cnqt4vfpugcu74wflc578pcmevith66aeorbca4jnxpwtyzcl05e66t9sn1ulnb41rv00unqr6fbzqsiufkfnqn5hva3nkfp7ghusjelblnqcuvxkknddkdq5gmiz4858ux1zccljwospxmwyx8homwl8nbbhmsu85ctzkiy3wwa2i2b0nhvs9df4k7dlpvbby1oicilso1u8xkw8wqv8c8jxm54hemkj9id8wfdj8lqds4rn4s7dis0caw7xem1f1csmmlmffaas7771ef7vkm69ddei1ig7u6vriheakcr81twj7x83p2k48b9mq2vktkutun84srxkugovml2gtc7aca9lopxzyp21s6fvcgld0azk1c9oxv6wiawxgyn5w19fzkf2y7nbfsgii32w0uztzgvy6e51xok1shv8c7pl4o7w1208h5a1w4rmjvxjq84i5y4w6c1ql8lb3ticaovjuu2ordec2hpo48e8klmam6moa64uwqzivhqqgdvdyenpd5yxefj216ygkjck38742x1pqrarzvshk6mky8ilm9e8etbf63g7arqmmhqzr7ragzxk5vm5pkkvcnm837uikx612xue35rqjhq2653frx8gtaky6ly4lcuopz8km43tyrv1ncd9mc75vqdixdz0xpw2xzbyii40qk27xxgljt6je59crma10r5kbgi0r92euq82kbvqqqyvdl1qgpgg5r0hy1372k46lswre277ms8yq837vdu3x9wjooxeq02s06kzoysuiq2nm4g5keqbjuzvxif6d3xkwxfw69g93265liu0i8t3klo2m0i8btk8pvj6ou1gcw6tk2gsnke7ufbiielqkdvur34jh6usqtd7a82qa7xeqd892e4036495uc7nkzsw5crj0n80xgn0qp2hlhe4jzgoo9kkkeriqntff1z3s7zney2364rtwmrm5o9m5u1jx5oqp4b9jp77dw3z7btojem1mkkh4sicj6nwyrbvynzti1kthgpgqzzrmlueq6bs33x6ozs15ve1zbib58wui0osx48noyejwm9prgir04lgagh6g1fint1gixk3b5s0o5ly47aoqr5lhdevp67k18j4207sgui52ilngzoyqrgdx2adv451lk0jw66rs560z0kkgn2jwxd63a7x71my7tjay7iigvsd5d6s47t192mhblpuni47etqsnd9n9o6ccpuoclvikyvsd53ql5rxbnp6lveobfcj3swl8i5c1muqudizha8kunmemc4auzr05b1y6dsmiiy5e7132j17pmrfxe3l77igxxu7lsqqqsbf6no1pjqj0lesiljhzdnfnayujq7xhqdjue4d8qf4q1llpx4y4ho0zzcl1484wl8jl4jqaeii6t5ti73ukxc26lnuuki7oubexu8a8puv06zf9kldlv0a6qw9jtj0gioeeav9argloscmeddj4vn91098ucl7tqd9ssvg23uod2anq4mwxp1uvrkg7oy2gnsw5420u5txb2eudhzvo6z23t7md9nswrdeehaw89hmrzoomtplc5j8ixqrekou2ry6ze2i6tfzu6dk500645yfc72mtui9bqtwcaidrwifvl9dgd1e5z1cbgem2ud0fk5xcjga96cfu6vsdko4y4a8rcyblfbnb6iamtiulg1cq8sfk63lvtfjsgh8fvfhye7yebrozhxz2sh6w0yol19o96pilnbcfgdt9n2sppq0x3x4xhacq1h8c392xuyrtjeq4m1dhcgbp59ww7ldluhoc1681nhxa1ge1rabvrfinh3fj9zgozkjwnjcr5gdfuc9qkub2kq7f9yxkvabciatx3vx2vj01i3rx4t7ue7li6uudot0cdgj7zduowg48oo1govqog94m8homfnziz1n2t707bx4eh31yiw2atiizuqc3tdbfycwu8mujo1707pzb6h76mahgmbyj3z3qvsf9230f440g5u34u8lbijjq4woavay0ecv9m82fjktmwruen22ospap0k5qzqeyzvmq12hqaupeuqtuw72q3o0ylz3t9onbvf7t7n4b9iih469gdglz9yccsjxtqtl71g0r5uv6ya7mt1m1pq8nhmlw27a3ykr07gr68mjji1dlnr4qeu8s0c1tfatx5781pd8rlyenry45xseoy13per45p3953bhj0zr6gd8ywi79ngajq9j4il4caegz505qvcssiu62nx5m17d8a2r5rr9i5vnhaq09ncmroknt50g8ioc4sps7ep3w9aa8s0rzquf6h0fejp96mz5gfl3asvlc9ifukuqbht3q084bmdx9l0bl8q5hx565wuqe768poqpruekpyke9v9d6lqdjd7kqoq9pftv9kiqmmk4b1ljr5sg5q0xkt5afopopvmq3tugmwfeq7xk2zy38g06bmzpruyzd68eevchx4ctjt6uxbss3luldp287lhotolab8vrid8tc307hyr5tolq3kbaf72gpdnyjnozigff3f2d7ybgdh89luo73zfj12tf7pnkhsj1m2u9b9pa8hk6lw4khjncfvwfndykwx6ztbd37ruzg1ajjtu6qqfe03t7h24q15jp8tedymiaqkgm41aed4di1fb566e5pglqa6u7umiyep8ph5p4h7hwx20kp7mmeoszf64eii1i3cmn0igwt9p9absujyhh9esfzxy64nulp4tc8jor9q3t37p1ucougatbolbepb09v7ime4e6epf6w5hobz6bdk3n4um4m92h3h9dfg822op2tjwlbcazzgo38dufbnp2ee2dyw38701mj2e5l36g0ss8t493bc0h5j90vkhl0vptvbydwj0b7x467u5f90prcqq1iqavscdeqitnzw3jkhdlcskj422ng8uuloh3o049ujtr7zhl9c3iqnfq4t4ugcycb8uz8bfxcck82axldmqnd4dfu1cp2vvg4ja5t4jyflb81r3h6nh4z5lvsllh05expe3tmw9ay2klpzehvgartep84mrqntrzx12r3civnluh9ojyld7uvazg62qed6o200cmsjoku3ducssvziz3wryuqbogsc5t9ubqvbgz4ibsz0nqv0kg5xmern2obnxoy06o1lf408e25wylbzviy9d08t7o8uhtix4qb3li273gvg5c7qwrtv7no7o81mrecnmxlioxgdffp6k1gleogwtc89c0klujkk9tpiho7lyzg0a5lrztfwonx1x0ciipq9t1nt2whl78k5hmstws9bc8qoq22rdogmlshka85mtcxwk8ffvqrhygl6spgul0jgu4vujw74kfsp6yam9mxvjr2dth2g1s8797js352uwaftd98ly4geuktao5b8o4b17ls85cxqdxg14t6qpwbmiszqum3k00erwg11m1vrlybdy24gjxq83kn63aa4lnwm6ho9xtei5h51g77mkukr78kazfqijpuajm0ky0p9n8pexj1503qj7hr18txse30wxgxvm92b0vrkz0nantvmbrza7qu0yc44mksginb5f5x9j2urffch9k5503rlu56948twhh9mmuemxdme9dqom42yitepyvgp7i5fpv5w6nh7v7n028ym7dyw9w7zz4tqw5k2elx5urptwwsgmk6xuv1yn5445mtlny1y9kf9bh2zhgbw21ttgj1ubiaa85msrxru7ilsytx94phoz6v8o1z60k6gw0ihzqq7e4q0ncfne7yzuj8at7zmh2v8j74hlv9q6odw0lbc4cjhh5hu21jo6dyghqgn6y3cr8c35z37h1461vbgbscjezbtqwuks14kbyg445owf1nvrm199yrwemi8w5ucu6tytetcvjxp0d6mxkgce4srlhayyqiwz8bfz47xdkj4kdiaeb4aoqrv76t1mum2jgopk1nzqcfgwhcimrw8xkfkf223kpmjqf1vj710aeugj8c9kvox4q003pdhkzx0kb2a4rohp44ksbuweh9edw9r9s0jhijewbhc50v8pxwy2t0d3rikw65qdz64mzzgddbydg93hgt3i6w5vre8zx0qlh4rezxln3fsyb3xc4p2gesf3ezujrn7tgbm8k3clii01qupdhlz9mkjw9kgob4r7m9ii18t40cjj9hegfjvs29o8p9f62gr2qy39duc96x3nie45yztcob8p2om135m9gxkzts4zgtorg79o2exxt6pufpy7jcsi3eku5xhjf2rvnp6o9p1c2jn7ky7km4qteftgayppe46fr0a2nv4btxg1doyknskrx3u9ixuoq1n27gg34xgutukx7uiypschftbtg8zevvnkhg4ats9by0vce3tjvtb3zoa7jde5t6dpjanm2vwjgjx3t7dbfvzv4e8eu4f1udrj2kunh603qhoub26qge38pyyk946olwg19gprpkidfku50km3q8jgc9ya736l4zw2hc6tqcaoith8yntzrbhgi52xzoo3c676axz7vhwc2m1leohyr5rh60571fbb3yua23e8vb9whae19oqf7yo8rstr2yixw6zwj4bguxhfvy8w2oc887mzyhs5vqnjtkqydavryqm18epuyuvqfnrqw8hkt6ussdlj6akuxetj8r9o7465nhepfjdzwg10ot6sp6zwc0uy5uz9lrgjjaif8y7dnimq50umdl8i0x27rlfraj0l4ajwatre4mqtr5a4wvhbi6ls6hxk51akk9pry40cw6yai4yrhwl5vasj5z8ydg4v8i71lspmty4ugytb1tpvu36g2jjzv23ygd29dwpe7kd0b23ujqz95wi9ikgakttxdhgp5mlp3qjb5gm5b450zte4sdawz1o6ij1jn7r4y18w3n41a9rcf7qjw5bj3hmmcm52tyjqp24edsy63x7xcpnj8rfy1jcegf4a26is99govb2pwws1fy7v1ktdkx2eicl9zuyhas2slr83s01i7h8haymjv32s6zl5ry7g713x8za5wqtr9wfky0o12kf7dsn392p26wwp4qgmve0jgonc7kzx9gv64gkstuhk6puzn5yromythgdvxambm83yig2r207xbdtvbcysekh3sxeccjdehaq40gfygn0z7hgip3fvtens53mgi4n7hwwj8ixqu88a8l3h2ztbtr1v4aip01pqfycckkmo0u6huvxnx3zhr9dlzssqa88j1kh95bztmw8uegga04ok0jn1r87vgurnbgapolslkqvx23g1ts2h2dz4v0qbbyj0q5upofyr3gbxq9hsyh9hu3q0uj8bzgytovbhtpavjgxtdc3ji8f52z0lnxtt8exhxymb4yje8aqoue5394b87wioxwg8p6culvpll6ye12ysrktk94hsjimuaubhp2rrai1a2nhmz37d8j61m8ikjurgd1e7orjf1yer0p4fkjqzj0zr9lyyms";

/// Pins: Create Entity of a `ListRelationship` whose second member is a URN of over 6,000 octets answers 201, the
/// members read back unchanged as Relationship Objects, and an equality on the long member selects the Entity
/// Clause: 4.6.2, Table 5.2.37-1, 4.9
#[gtest]
#[tokio::test]
async fn n_ent_cre_9016_a_list_relationship_member_of_six_thousand_octets_round_trips() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let reference_id = namespace.urn("LongReference", "list")?;
    let reference_type = namespace.entity_type("LongReference")?;
    let short_id = namespace.urn("StopTimeUpdate", "short")?;
    let long_id = format!("urn:ngsi-ld:StopTimeUpdate:{namespace}-{LONG_TAIL}").parse::<EntityId>()?;
    let reference = Entity::builder().id(reference_id.clone()).entity_type(reference_type.clone()).build()?;
    // The model writes the members of an `objectList` as objects, so the list of plain strings is written as text.
    let served_by: RawJson = format!(r#"{{"type": "ListRelationship", "objectList": ["{short_id}", "{long_id}"]}}"#).parse()?;
    let document = UncheckedDocument::from_entity(&reference, Lossless::Normalized).set_member("servedBy", &served_by);
    let request = broker.unchecked(&CreateEntity::builder().entity(&reference).build())?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&reference_id)
        .send()
        .await?;
    expect_that!(
        retrieved.body(),
        has_attribute(
            "servedBy",
            attribute_of(AttributeKind::ListRelationship)
                .list_length(2)
                .list_object(0, short_id.clone())
                .list_object(1, long_id.clone())
        )
    );
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&reference_type)
        .locality(Locality::Local)
        .ids(vec![reference_id.clone()])
        .q(q::attr("servedBy").eq(long_id.to_string().as_str()))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&reference_id)));
    Ok(())
}
