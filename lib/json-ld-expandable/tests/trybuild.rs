#[test]
fn json_ld_expandable() {
    let t = trybuild::TestCases::new();
    t.pass("tests/expand/pass_*.rs");
    t.compile_fail("tests/expand/fail_*.rs");
}
