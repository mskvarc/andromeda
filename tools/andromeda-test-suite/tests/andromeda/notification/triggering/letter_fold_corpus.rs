//! The letter corpus of the case fold notification tests: 1201 code points of the Latin, Greek and Cyrillic blocks
//! and the Ohm, Kelvin and Angstrom signs, each created as a subject, and the check that a Subscription on
//! `name~="(?i)^<letter>$"` notifies exactly the letter's case fold set and that a query carrying the same q selects
//! that set.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, notification_sink::NotificationSink, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, Input, Lossless, NgsiLdMediaType},
    query::q,
    subscription::{Endpoint, NotificationParams, NotificationTrigger, Subscription},
};
use std::ops::{Range, RangeInclusive};

/// The code point runs of the corpus, in subject order: subject `n` is the `n`-th letter of these runs.
const LETTER_RUNS: [RangeInclusive<char>; 6] = [
    'A'..='~',
    '\u{a0}'..='\u{24f}',
    '\u{370}'..='\u{52f}',
    '\u{1e00}'..='\u{1eff}',
    '\u{2126}'..='\u{2126}',
    '\u{212a}'..='\u{212b}',
];

/// Subjects per Batch Entity Create.
const BATCH: usize = 250;

/// Letters the pattern writes after a backslash.
const ESCAPED: [char; 8] = ['[', '\\', ']', '^', '{', '|', '}', '~'];

/// The case fold sets of the corpus that hold more than one letter, separated by spaces; every other letter folds to
/// itself alone.
const FOLD_SETS: [&str; 17] = [
    "Aa Bb Cc Dd Ee Ff Gg Hh Ii Jj Kk\u{212a} Ll Mm Nn Oo Pp Qq Rr Ssſ Tt Uu Vv Ww Xx Yy Zz µΜμ Àà Áá Ââ",
    "Ãã Ää Åå\u{212b} Ææ Çç Èè Éé Êê Ëë Ìì Íí Îî Ïï Ðð Ññ Òò Óó Ôô Õõ Öö Øø Ùù Úú Ûû Üü Ýý Þþ ßẞ ÿŸ Āā Ăă",
    "Ąą Ćć Ĉĉ Ċċ Čč Ďď Đđ Ēē Ĕĕ Ėė Ęę Ěě Ĝĝ Ğğ Ġġ Ģģ Ĥĥ Ħħ Ĩĩ Īī Ĭĭ Įį Ĳĳ Ĵĵ Ķķ Ĺĺ Ļļ Ľľ Ŀŀ Łł Ńń Ņņ Ňň",
    "Ŋŋ Ōō Ŏŏ Őő Œœ Ŕŕ Ŗŗ Řř Śś Ŝŝ Şş Šš Ţţ Ťť Ŧŧ Ũũ Ūū Ŭŭ Ůů Űű Ųų Ŵŵ Ŷŷ Źź Żż Žž ƀɃ Ƃƃ Ƅƅ Ƈƈ Ƌƌ Ǝǝ Ƒƒ",
    "ƕǶ Ƙƙ ƚȽ ƞȠ Ơơ Ƣƣ Ƥƥ Ƨƨ Ƭƭ Ưư Ƴƴ Ƶƶ Ƹƹ Ƽƽ ƿǷ Ǆǅǆ Ǉǈǉ Ǌǋǌ Ǎǎ Ǐǐ Ǒǒ Ǔǔ Ǖǖ Ǘǘ Ǚǚ Ǜǜ Ǟǟ Ǡǡ Ǣǣ Ǥǥ Ǧǧ Ǩǩ",
    "Ǫǫ Ǭǭ Ǯǯ Ǳǲǳ Ǵǵ Ǹǹ Ǻǻ Ǽǽ Ǿǿ Ȁȁ Ȃȃ Ȅȅ Ȇȇ Ȉȉ Ȋȋ Ȍȍ Ȏȏ Ȑȑ Ȓȓ Ȕȕ Ȗȗ Șș Țț Ȝȝ Ȟȟ Ȣȣ Ȥȥ Ȧȧ Ȩȩ Ȫȫ Ȭȭ Ȯȯ Ȱȱ",
    "Ȳȳ Ȼȼ Ɂɂ Ɇɇ Ɉɉ Ɋɋ Ɍɍ Ɏɏ Ͱͱ Ͳͳ Ͷͷ ͻϽ ͼϾ ͽϿ Ϳϳ Άά Έέ Ήή Ίί Όό Ύύ Ώώ Αα Ββϐ Γγ Δδ Εεϵ Ζζ Ηη Θθϑϴ Ιι Κκϰ",
    "Λλ Νν Ξξ Οο Ππϖ Ρρϱ Σςσ Ττ Υυ Φφϕ Χχ Ψψ Ωω\u{2126} Ϊϊ Ϋϋ Ϗϗ Ϙϙ Ϛϛ Ϝϝ Ϟϟ Ϡϡ Ϣϣ Ϥϥ Ϧϧ Ϩϩ Ϫϫ Ϭϭ Ϯϯ ϲϹ",
    "Ϸϸ Ϻϻ Ѐѐ Ёё Ђђ Ѓѓ Єє Ѕѕ Іі Її Јј Љљ Њњ Ћћ Ќќ Ѝѝ Ўў Џџ Аа Бб Вв Гг Дд Ее Жж Зз Ии Йй Кк Лл Мм Нн Оо",
    "Пп Рр Сс Тт Уу Фф Хх Цц Чч Шш Щщ Ъъ Ыы Ьь Ээ Юю Яя Ѡѡ Ѣѣ Ѥѥ Ѧѧ Ѩѩ Ѫѫ Ѭѭ Ѯѯ Ѱѱ Ѳѳ Ѵѵ Ѷѷ Ѹѹ Ѻѻ Ѽѽ Ѿѿ",
    "Ҁҁ Ҋҋ Ҍҍ Ҏҏ Ґґ Ғғ Ҕҕ Җҗ Ҙҙ Ққ Ҝҝ Ҟҟ Ҡҡ Ңң Ҥҥ Ҧҧ Ҩҩ Ҫҫ Ҭҭ Үү Ұұ Ҳҳ Ҵҵ Ҷҷ Ҹҹ Һһ Ҽҽ Ҿҿ Ӏӏ Ӂӂ Ӄӄ Ӆӆ Ӈӈ",
    "Ӊӊ Ӌӌ Ӎӎ Ӑӑ Ӓӓ Ӕӕ Ӗӗ Әә Ӛӛ Ӝӝ Ӟӟ Ӡӡ Ӣӣ Ӥӥ Ӧӧ Өө Ӫӫ Ӭӭ Ӯӯ Ӱӱ Ӳӳ Ӵӵ Ӷӷ Ӹӹ Ӻӻ Ӽӽ Ӿӿ Ԁԁ Ԃԃ Ԅԅ Ԇԇ Ԉԉ Ԋԋ",
    "Ԍԍ Ԏԏ Ԑԑ Ԓԓ Ԕԕ Ԗԗ Ԙԙ Ԛԛ Ԝԝ Ԟԟ Ԡԡ Ԣԣ Ԥԥ Ԧԧ Ԩԩ Ԫԫ Ԭԭ Ԯԯ Ḁḁ Ḃḃ Ḅḅ Ḇḇ Ḉḉ Ḋḋ Ḍḍ Ḏḏ Ḑḑ Ḓḓ Ḕḕ Ḗḗ Ḙḙ Ḛḛ Ḝḝ",
    "Ḟḟ Ḡḡ Ḣḣ Ḥḥ Ḧḧ Ḩḩ Ḫḫ Ḭḭ Ḯḯ Ḱḱ Ḳḳ Ḵḵ Ḷḷ Ḹḹ Ḻḻ Ḽḽ Ḿḿ Ṁṁ Ṃṃ Ṅṅ Ṇṇ Ṉṉ Ṋṋ Ṍṍ Ṏṏ Ṑṑ Ṓṓ Ṕṕ Ṗṗ Ṙṙ Ṛṛ Ṝṝ Ṟṟ",
    "Ṡṡẛ Ṣṣ Ṥṥ Ṧṧ Ṩṩ Ṫṫ Ṭṭ Ṯṯ Ṱṱ Ṳṳ Ṵṵ Ṷṷ Ṹṹ Ṻṻ Ṽṽ Ṿṿ Ẁẁ Ẃẃ Ẅẅ Ẇẇ Ẉẉ Ẋẋ Ẍẍ Ẏẏ Ẑẑ Ẓẓ Ẕẕ Ạạ Ảả Ấấ Ầầ Ẩẩ Ẫẫ",
    "Ậậ Ắắ Ằằ Ẳẳ Ẵẵ Ặặ Ẹẹ Ẻẻ Ẽẽ Ếế Ềề Ểể Ễễ Ệệ Ỉỉ Ịị Ọọ Ỏỏ Ốố Ồồ Ổổ Ỗỗ Ộộ Ớớ Ờờ Ởở Ỡỡ Ợợ Ụụ Ủủ Ứứ Ừừ Ửử",
    "Ữữ Ựự Ỳỳ Ỵỵ Ỷỷ Ỹỹ Ỻỻ Ỽỽ Ỿỿ",
];

/// The `q` value of the Subscription and the query on `letter`: `"(?i)^<letter>$"`, quotes included.
fn pattern(letter: char) -> String {
    let escape = if ESCAPED.contains(&letter) { "\\" } else { "" };
    format!("\"(?i)^{escape}{letter}$\"")
}

/// The subject numbers of the letters `letter` folds with, itself included.
fn fold_set(corpus: &[char], letter: char) -> Vec<usize> {
    let members = FOLD_SETS
        .iter()
        .flat_map(|row| row.split(' '))
        .find(|set| set.contains(letter))
        .map_or_else(|| vec![letter], |set| set.chars().collect());
    members
        .iter()
        .filter_map(|member| corpus.iter().position(|candidate| candidate == member))
        .collect()
}

/// Subscribes on the pattern of each letter of `letters` (subject numbers), creates the whole corpus, then checks that
/// each Subscription notified exactly the letter's fold set and that a query on the same pattern selects it.
///
/// # Errors
///
/// Returns the first request, decoding or building failure.
pub(crate) async fn notifies_each_fold_set(letters: Range<usize>) -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let letter_type = namespace.entity_type("Letter")?;
    let corpus = LETTER_RUNS.into_iter().flatten().collect::<Vec<_>>();
    let cases = corpus.get(letters).or_fail()?;
    for (index, &letter) in cases.iter().enumerate() {
        let subscription = Subscription::builder()
            .id(namespace.subscription_id(index)?)
            .entities([letter_type.as_str()])
            .notification_triggers([NotificationTrigger::EntityCreated])
            .q(q::attr("name").matches(pattern(letter).as_str()).build()?)
            .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(index)?).accept(NgsiLdMediaType::Json).build()?).build()?)
            .build()?;
        broker.subscriptions().create(&subscription).send().await?;
    }
    for (batch, letters) in corpus.chunks(BATCH).enumerate() {
        let entities = letters
            .iter()
            .enumerate()
            .map(|(offset, letter)| -> Result<Entity<Input>> {
                Ok(Entity::builder()
                    .id(namespace.urn("Subject", format!("{:04}", batch * BATCH + offset))?)
                    .entity_type(letter_type.clone())
                    .property("name", letter.to_string().as_str())
                    .build()?)
            })
            .collect::<Result<Vec<_>>>()?;
        broker.entities().batch_create(&entities).send().await?.done()?;
    }
    for (index, &letter) in cases.iter().enumerate() {
        let subjects = fold_set(&corpus, letter);
        sink.wait_for_at_least(index, subjects.len()).await?;
        let notifications = sink.received(index).await?;
        expect_that!(notifications, len(eq(subjects.len())));
        let notified_ids = notifications
            .iter()
            .map(|received| received.entities(Lossless::Normalized))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flat_map(|notification| notification.data().iter().map(|entity| entity.id.clone()).collect::<Vec<_>>())
            .collect::<Vec<_>>();
        let subject_ids = subjects
            .iter()
            .map(|subject| namespace.urn("Subject", format!("{subject:04}")))
            .collect::<Result<Vec<_>, _>>()?;
        expect_that!(notified_ids, superset_of(subject_ids.iter().collect::<Vec<_>>()));
        let page = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .query()
            .entity_type(&letter_type)
            .limit(1000)
            .q(q::attr("name").matches(pattern(letter).as_str()))
            .send()
            .await?;
        expect_that!(page.items(), len(eq(subjects.len())));
        for subject_id in &subject_ids {
            expect_that!(page.items(), contains(has_id(subject_id)));
        }
    }
    Ok(())
}
