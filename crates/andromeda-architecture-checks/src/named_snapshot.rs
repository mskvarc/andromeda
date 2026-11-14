//! The one way a test of the workspace takes an insta snapshot.
//!
//! A snapshot is a file the repository checks in beside the crate whose test
//! takes it: `snapshots/<name>.snap` at the root of that crate, so a crate's
//! snapshots sit together whatever module their tests live in. The name is
//! given in kebab-case and insta prepends no module path, so the file follows
//! the repository's naming of files and the name is unique within the crate.
//! The header of each file names the test's source file and leaves out the
//! asserted expression, which says nothing the source does not.

/// Assert that the text `$text` matches the insta snapshot `$name` of the
/// calling crate, under the settings this module describes.
///
/// Expands to `insta::with_settings!` around `insta::assert_snapshot!`, so the
/// calling crate takes `insta` as a dev-dependency.
#[macro_export]
macro_rules! assert_named_snapshot {
    ($name:expr, $text:expr $(,)?) => {
        ::insta::with_settings!(
            {
                prepend_module_to_snapshot => false,
                omit_expression => true,
                snapshot_path => ::std::concat!(::std::env!("CARGO_MANIFEST_DIR"), "/snapshots"),
            },
            {
                ::insta::assert_snapshot!($name, $text);
            }
        )
    };
}
