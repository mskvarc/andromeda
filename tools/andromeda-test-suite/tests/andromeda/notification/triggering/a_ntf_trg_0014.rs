//! Every pattern of a corpus notifies the subjects the same query selects.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    notification_sink::NotificationSink,
    outcome::Done,
    problem::answered,
    raw_call::{call, target},
    unchecked_page::entities,
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::{Entity, Input};
use serde_json::{Value, json};

/// A `q` pattern and the numbers of the subjects it selects.
struct Pattern {
    /// The `q` of the subscription, as a JSON string holds it.
    q: &'static str,
    /// The numbers of the subjects the pattern selects, ascending.
    notified: &'static [usize],
}

/// The `name` of each subject, by number.
const NAMES: [&str; 114] = [
    "\u{10d}",
    "\u{10c}",
    "c",
    "C",
    "\u{10c}er",
    "\u{10c}ER",
    "\u{10d}Er",
    "cer",
    "\u{10d}ebela",
    "\u{10c}ebela",
    "\u{10c}EBELA",
    "\u{10c}Ebela",
    "cebela",
    "a",
    "A",
    "z",
    "Z",
    "\u{142}",
    "\u{141}",
    "\u{17e}",
    "\u{17d}",
    "\u{1c6}",
    "\u{1c5}",
    "\u{1c4}",
    "k",
    "K",
    "\u{212a}",
    "s",
    "S",
    "\u{17f}",
    "\u{3c3}",
    "\u{3c2}",
    "\u{3a3}",
    "\u{df}",
    "\u{1e9e}",
    "SS",
    "i",
    "I",
    "\u{130}",
    "\u{131}",
    "\u{151}",
    "\u{150}",
    "\u{133}",
    "\u{132}",
    "\u{e9}",
    "\u{c9}",
    "\u{f6}",
    "\u{d6}",
    "\u{e4}",
    "\u{c4}",
    "\u{fc}",
    "\u{dc}",
    "\u{17d}alec",
    "\u{17e}alec-12",
    "\u{17d}ALEC-X",
    "Stra\u{df}e",
    "STRA\u{1e9e}E",
    "STRASSE",
    "\u{3c3}\u{3bf}\u{3c6}\u{3b9}\u{3b1}",
    "\u{3a3}\u{39f}\u{3a6}\u{399}\u{391}",
    "\u{3c3}\u{3bf}\u{3c6}\u{3b9}\u{3ac}",
    "[",
    "\\",
    "]",
    "^",
    "_",
    "`",
    "-",
    ".",
    "/",
    "\u{ff}",
    "\u{100}",
    "\u{451}",
    "\u{401}",
    "\u{44f}",
    "\u{42f}",
    "\u{450}",
    "B",
    "1",
    " ",
    "ab",
    "abe",
    "cde",
    "Abe",
    "aBc",
    "abc",
    "aa",
    "aaa",
    "a\nb",
    "b\na",
    "prvi\n\u{10c}ebela",
    "prvi\n\u{10d}ebela",
    "\u{160}kofja Loka",
    "urn:x-ngsi:Vehicle:A1",
    "Mercedes",
    "Merc.edes",
    "\u{663}",
    "\u{662}\u{660}\u{662}\u{666}",
    "\u{ff11}",
    "\u{b2}",
    "\u{bd}",
    "\u{a0}",
    "\u{3000}",
    "\u{200b}",
    "\t",
    "\u{160}kofja\u{a0}Loka",
    "\u{160}kofja\u{3000}Loka",
    "e\u{301}",
    "a_1",
    "urn:x-ngsi:\u{10c}ebela:\u{663}",
    "urn:x-ngsi:\u{10d}ebela:12",
    "urn:x-ngsi:\u{10d}ebela:x",
    "\u{395}\u{3bb}\u{3bb}\u{3ac}\u{3b4}\u{3b1}",
    "\u{142}\u{f3}d\u{17a}",
];

/// The patterns, each notified through the endpoint of its index.
const PATTERNS: [Pattern; 61] = [
    Pattern {
        q: "name~=\"(?i)\u{10d}er\"",
        notified: &[4, 5, 6],
    },
    Pattern {
        q: "name~=\"(?i)^\u{10d}ebela$\"",
        notified: &[8, 9, 10, 11],
    },
    Pattern {
        q: "name~=\"(?i)[a-\u{17e}]\"",
        notified: &[
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 24, 25, 26, 27, 28, 29, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44,
            45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 70, 71, 77, 80, 81, 82, 83, 84, 85, 86, 87, 88, 89, 90, 91, 92, 93, 94, 95, 99, 100, 101, 105,
            106, 107, 108, 109, 110, 111, 113,
        ],
    },
    Pattern {
        q: "name~=\"(?i)[\u{10d}-\u{17e}]\"",
        notified: &[
            0, 1, 4, 5, 6, 8, 9, 10, 11, 17, 18, 19, 20, 38, 39, 40, 41, 42, 43, 52, 53, 54, 70, 90, 91, 92, 105, 106, 109, 110, 111, 113,
        ],
    },
    Pattern {
        q: "name~=\"(?i)k\"",
        notified: &[24, 25, 26, 92, 105, 106],
    },
    Pattern {
        q: "name~=\"(?i)s\"",
        notified: &[27, 28, 29, 35, 55, 56, 57, 93, 94, 95, 109, 110, 111],
    },
    Pattern {
        q: "name~=\"(?i)\u{3c3}\"",
        notified: &[30, 31, 32, 58, 59, 60],
    },
    Pattern {
        q: "name~=\"(?i)\u{1c6}\"",
        notified: &[21, 22, 23],
    },
    Pattern {
        q: "name~=\"(?i)[^\u{10d}]\"",
        notified: &[
            2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40,
            41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77,
            78, 79, 80, 81, 82, 83, 84, 85, 86, 87, 88, 89, 90, 91, 92, 93, 94, 95, 96, 97, 98, 99, 100, 101, 102, 103, 104, 105, 106, 107, 108, 109, 110, 111,
            112, 113,
        ],
    },
    Pattern {
        q: "name~=\"(?i)^[^\u{10d}\u{161}]+$\"",
        notified: &[
            2, 3, 7, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45,
            46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 78, 79, 80, 81, 82,
            83, 84, 85, 86, 87, 88, 89, 93, 94, 95, 96, 97, 98, 99, 100, 101, 102, 103, 104, 107, 108, 112, 113,
        ],
    },
    Pattern {
        q: "name~=\"(?i)\u{df}\"",
        notified: &[33, 34, 55, 56],
    },
    Pattern {
        q: "name~=\"(?i)i\"",
        notified: &[36, 37, 90, 91, 93, 109, 110, 111],
    },
    Pattern {
        q: "name~=\"(?i)\u{151}\"",
        notified: &[40, 41],
    },
    Pattern {
        q: "name~=\"(?i)\u{142}\"",
        notified: &[17, 18, 113],
    },
    Pattern {
        q: "name~=\"(?i)\u{133}\"",
        notified: &[42, 43],
    },
    Pattern {
        q: "name~=\"(?i)[[:alpha:]]\"",
        notified: &[
            2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 24, 25, 26, 27, 28, 29, 35, 36, 37, 52, 53, 54, 55, 56, 57, 77, 80, 81, 82, 83, 84, 85, 86, 87,
            88, 89, 90, 91, 92, 93, 94, 95, 105, 106, 107, 108, 109, 110, 111, 113,
        ],
    },
    Pattern {
        q: "name~=\"(?i)[[:upper:]]+\"",
        notified: &[
            2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 24, 25, 26, 27, 28, 29, 35, 36, 37, 52, 53, 54, 55, 56, 57, 77, 80, 81, 82, 83, 84, 85, 86, 87,
            88, 89, 90, 91, 92, 93, 94, 95, 105, 106, 107, 108, 109, 110, 111, 113,
        ],
    },
    Pattern {
        q: "name~=\"(?i)\u{f6}|\u{e4}|\u{fc}\"",
        notified: &[46, 47, 48, 49, 50, 51],
    },
    Pattern {
        q: "name~=\"(?i)^\u{17d}ALEC(?:-[0-9]+)?$\"",
        notified: &[52, 53],
    },
    Pattern {
        q: "name~=\"(?i)^stra\u{df}e$\"",
        notified: &[55, 56],
    },
    Pattern {
        q: "name~=\"(?i)\u{3a3}\u{39f}\u{3a6}\u{399}\u{391}\"",
        notified: &[58, 59],
    },
    Pattern {
        q: "name~=\"(?i:\u{10d})er\"",
        notified: &[4],
    },
    Pattern {
        q: "name~=\"^(?i:\u{10d})ebela$\"",
        notified: &[8, 9],
    },
    Pattern {
        q: "name~=\"a(?i)b(?-i)c\"",
        notified: &[84, 85],
    },
    Pattern {
        q: "name~=\"(?i)^\u{10d}e(?-i)bela$\"",
        notified: &[8, 9, 11],
    },
    Pattern {
        q: "name~=\"(?i)(ab|cd)(?-i)e\"",
        notified: &[81, 82, 83],
    },
    Pattern {
        q: "name~=\"(?im)^prvi.\u{10d}ebela$\"",
        notified: &[90, 91],
    },
    Pattern {
        q: "name~=\"(?m)^b$\"",
        notified: &[88, 89],
    },
    Pattern {
        q: "name~=\"(?m)a$\"",
        notified: &[8, 9, 11, 12, 13, 86, 87, 88, 89, 90, 91, 92, 105, 106],
    },
    Pattern {
        q: "name~=\"(?x) \u{10d} e r\"",
        notified: &[],
    },
    Pattern {
        q: "name~=\"(?s)a.b\"",
        notified: &[88],
    },
    Pattern {
        q: "name~=\"[\u{10c}-\u{10d}]\"",
        notified: &[0, 1, 4, 5, 6, 8, 9, 10, 11, 90, 91, 109, 110, 111],
    },
    Pattern {
        q: "name~=\"[a-\u{17e}]\"",
        notified: &[
            0, 1, 2, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 15, 17, 18, 19, 20, 24, 27, 33, 36, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54,
            55, 70, 71, 80, 81, 82, 83, 84, 85, 86, 87, 88, 89, 90, 91, 92, 93, 94, 95, 99, 100, 101, 105, 106, 107, 108, 109, 110, 111, 113,
        ],
    },
    Pattern {
        q: "name~=\"[Z-a]\"",
        notified: &[
            8, 9, 11, 12, 13, 16, 52, 53, 55, 61, 62, 63, 64, 65, 66, 80, 81, 84, 85, 86, 87, 88, 89, 90, 91, 92, 105, 106, 108, 109, 110, 111,
        ],
    },
    Pattern {
        q: "name~=\"[\u{ff}-\u{17e}]\"",
        notified: &[
            0, 1, 4, 5, 6, 8, 9, 10, 11, 17, 18, 19, 20, 38, 39, 40, 41, 42, 43, 52, 53, 54, 70, 71, 90, 91, 92, 105, 106, 109, 110, 111, 113,
        ],
    },
    Pattern {
        q: "name~=\"[\u{410}-\u{44f}]\"",
        notified: &[74, 75],
    },
    Pattern {
        q: "name~=\"[[:upper:]]+\"",
        notified: &[3, 5, 6, 10, 11, 14, 16, 25, 28, 35, 37, 54, 55, 56, 57, 77, 83, 84, 92, 93, 94, 95, 105, 106],
    },
    Pattern {
        q: "name~=\"^urn:x-ngsi:Vehicle:.*\"",
        notified: &[93],
    },
    Pattern {
        q: "name~=\"a.b\"",
        notified: &[88],
    },
    Pattern {
        q: "name~=\"[]a]\"",
        notified: &[
            8, 9, 11, 12, 13, 52, 53, 55, 63, 80, 81, 84, 85, 86, 87, 88, 89, 90, 91, 92, 105, 106, 108, 109, 110, 111,
        ],
    },
    Pattern {
        q: "name~=\"[^]a]\"",
        notified: &[
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39,
            40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 64, 65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77,
            78, 79, 80, 81, 82, 83, 84, 85, 88, 89, 90, 91, 92, 93, 94, 95, 96, 97, 98, 99, 100, 101, 102, 103, 104, 105, 106, 107, 108, 109, 110, 111, 112,
            113,
        ],
    },
    Pattern {
        q: "name~=\"[a-]\"",
        notified: &[
            8, 9, 11, 12, 13, 52, 53, 54, 55, 67, 80, 81, 84, 85, 86, 87, 88, 89, 90, 91, 92, 93, 105, 106, 108, 109, 110, 111,
        ],
    },
    Pattern {
        q: "name~=\"[.-/]\"",
        notified: &[68, 69, 95],
    },
    Pattern {
        q: "name~=\"a{2,3}\"",
        notified: &[86, 87],
    },
    Pattern {
        q: "name~=\"(a*)*\"",
        notified: &[
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38,
            39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75,
            76, 77, 78, 79, 80, 81, 82, 83, 84, 85, 86, 87, 88, 89, 90, 91, 92, 93, 94, 95, 96, 97, 98, 99, 100, 101, 102, 103, 104, 105, 106, 107, 108, 109,
            110, 111, 112, 113,
        ],
    },
    Pattern {
        q: "name~=\"Merc\\.?edes\"",
        notified: &[94, 95],
    },
    Pattern {
        q: "name~=\"\\w\"",
        notified: &[
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38,
            39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 65, 70, 71, 72, 73, 74, 75, 76, 77, 78, 80, 81, 82, 83, 84,
            85, 86, 87, 88, 89, 90, 91, 92, 93, 94, 95, 96, 97, 98, 105, 106, 107, 108, 109, 110, 111, 112, 113,
        ],
    },
    Pattern {
        q: "name~=\"^\\w+$\"",
        notified: &[
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38,
            39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 55, 56, 57, 58, 59, 60, 65, 70, 71, 72, 73, 74, 75, 76, 77, 78, 80, 81, 82, 83, 84, 85, 86,
            87, 94, 96, 97, 98, 107, 108, 112, 113,
        ],
    },
    Pattern {
        q: "name~=\"\\W\"",
        notified: &[
            53, 54, 61, 62, 63, 64, 66, 67, 68, 69, 79, 88, 89, 90, 91, 92, 93, 95, 99, 100, 101, 102, 103, 104, 105, 106, 109, 110, 111,
        ],
    },
    Pattern {
        q: "name~=\"\\d\"",
        notified: &[53, 78, 93, 96, 97, 98, 108, 109, 110],
    },
    Pattern {
        q: "name~=\"^\\d+$\"",
        notified: &[78, 96, 97, 98],
    },
    Pattern {
        q: "name~=\"\\D\"",
        notified: &[
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38,
            39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75,
            76, 77, 79, 80, 81, 82, 83, 84, 85, 86, 87, 88, 89, 90, 91, 92, 93, 94, 95, 99, 100, 101, 102, 103, 104, 105, 106, 107, 108, 109, 110, 111, 112,
            113,
        ],
    },
    Pattern {
        q: "name~=\"\\s\"",
        notified: &[79, 88, 89, 90, 91, 92, 101, 102, 104, 105, 106],
    },
    Pattern {
        q: "name~=\"^\\S+\\s\\S+$\"",
        notified: &[88, 89, 90, 91, 92, 105, 106],
    },
    Pattern {
        q: "name~=\"[^\\s]\"",
        notified: &[
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38,
            39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75,
            76, 77, 78, 80, 81, 82, 83, 84, 85, 86, 87, 88, 89, 90, 91, 92, 93, 94, 95, 96, 97, 98, 99, 100, 103, 105, 106, 107, 108, 109, 110, 111, 112, 113,
        ],
    },
    Pattern {
        q: "name~=\"^[\\w\\-:]+$\"",
        notified: &[
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38,
            39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 65, 67, 70, 71, 72, 73, 74, 75, 76, 77, 78, 80, 81, 82, 83,
            84, 85, 86, 87, 93, 94, 96, 97, 98, 107, 108, 109, 110, 111, 112, 113,
        ],
    },
    Pattern {
        q: "name~=\"^[\\w.]+$\"",
        notified: &[
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38,
            39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 55, 56, 57, 58, 59, 60, 65, 68, 70, 71, 72, 73, 74, 75, 76, 77, 78, 80, 81, 82, 83, 84, 85,
            86, 87, 94, 95, 96, 97, 98, 107, 108, 112, 113,
        ],
    },
    Pattern {
        q: "name~=\"(?i)^\\w+$\"",
        notified: &[
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38,
            39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 55, 56, 57, 58, 59, 60, 65, 70, 71, 72, 73, 74, 75, 76, 77, 78, 80, 81, 82, 83, 84, 85, 86,
            87, 94, 96, 97, 98, 107, 108, 112, 113,
        ],
    },
    Pattern {
        q: "name~=\"(?i)^\u{10d}\\w*$\"",
        notified: &[0, 1, 4, 5, 6, 8, 9, 10, 11],
    },
    Pattern {
        q: "name~=\"^urn:x-ngsi:\\w+:\\d+$\"",
        notified: &[109, 110],
    },
    Pattern {
        q: "name~=\"(?m)^\\w+$\"",
        notified: &[
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38,
            39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 55, 56, 57, 58, 59, 60, 65, 70, 71, 72, 73, 74, 75, 76, 77, 78, 80, 81, 82, 83, 84, 85, 86,
            87, 88, 89, 90, 91, 94, 96, 97, 98, 107, 108, 112, 113,
        ],
    },
];

/// The identifier of subject `number` in the namespace.
fn subject_id(namespace: &Namespace, number: usize) -> String {
    namespace.urn_text("Subject", format!("{number:03}"))
}

/// The identifiers of the subjects `numbers` name, ascending.
fn subject_ids(namespace: &Namespace, numbers: &[usize]) -> Vec<String> {
    numbers.iter().map(|number| subject_id(namespace, *number)).collect()
}

/// Pins: For each of 61 q patterns (the case-insensitive option set, scoped and cleared, over letters with two,
/// three or no fold partners, POSIX bracket classes, the shorthand classes bare, bracketed, negated and folded, and
/// constructs carried unchanged) a Subscription with the `entityCreated` trigger notifies exactly the subjects, of
/// 114 created in one batch, that a query carrying the same q selects
/// Clause: 4.9, 5.8.6, 5.7.2.4
/// Recorded: clause 4.9 hands `RegExp` to IEEE 1003.2, which has no inline options and no shorthand classes;
/// Andromeda admits the case-insensitive option and the shorthand classes \d, \w and \s as extensions, folding case
/// by Unicode simple case folding and classifying by Unicode
#[gtest]
#[tokio::test]
async fn a_ntf_trg_0014_every_pattern_of_a_corpus_notifies_the_subjects_the_same_query_selects() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let subject_type = namespace.entity_type("Subject")?;
    // The patterns are regular expressions the `q` model may refuse, so the subscriptions are sent as written.
    for (index, pattern) in PATTERNS.iter().enumerate() {
        let body = json!({
            "id": namespace.urn_text("Subscription", index),
            "type": "Subscription",
            "entities": [{"type": subject_type.as_str()}],
            "notificationTrigger": ["entityCreated"],
            "q": pattern.q,
            "notification": {"endpoint": {"uri": sink.endpoint(index)?.to_string(), "accept": "application/json"}}
        });
        let created = broker
            .send_unchecked(json_request_to(&broker, Method::POST, "subscriptions", &body.to_string())?)
            .await;
        expect_that!(&created, answered(StatusCode::CREATED));
    }
    let subjects = NAMES
        .iter()
        .enumerate()
        .map(|(number, name)| -> Result<Entity<Input>> {
            Ok(Entity::builder()
                .id(namespace.urn("Subject", format!("{number:03}"))?)
                .entity_type(subject_type.clone())
                .property("name", *name)
                .build()?)
        })
        .collect::<Result<Vec<_>>>()?;
    broker.entities().batch_create(&subjects).send().await?.done()?;
    for (index, pattern) in PATTERNS.iter().enumerate() {
        let expected = subject_ids(&namespace, pattern.notified);
        if !pattern.notified.is_empty() {
            sink.wait_for_at_least(index, pattern.notified.len()).await?;
        }
        let notified = sink.received(index).await?;
        expect_that!(notified, len(eq(expected.len())));
        let mut notified_ids = notified
            .iter()
            .map(|notification| Ok(notification.json()?.pointer("/data/0/id").and_then(Value::as_str).or_fail()?.to_owned()))
            .collect::<Result<Vec<String>>>()?;
        notified_ids.sort();
        expect_that!(notified_ids, eq(&expected));
        let selected = call(
            &broker,
            Method::GET,
            &target("entities", &[("type", subject_type.as_str()), ("limit", "1000"), ("q", pattern.q)]),
            &[("Accept", "application/json")],
            None,
        )
        .await?;
        expect_that!(selected.status(), eq(StatusCode::OK));
        let page = entities(&selected)?;
        expect_that!(page, len(eq(expected.len())));
        let mut selected_ids = page.iter().map(|entity| entity.id.to_string()).collect::<Vec<String>>();
        selected_ids.sort();
        expect_that!(selected_ids, eq(&expected));
    }
    Ok(())
}
