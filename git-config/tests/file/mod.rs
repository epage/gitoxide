use std::borrow::Cow;

use bstr::{BStr, ByteSlice};

pub fn cow_str(s: &str) -> Cow<'_, BStr> {
    Cow::Borrowed(s.as_bytes().as_bstr())
}

#[test]
fn size_in_memory() {
    let actual = std::mem::size_of::<git_config::File<'_>>();
    assert!(
        actual <= 1040,
        "{} <= 1040: This shouldn't change without us noticing",
        actual
    );
}

mod open {
    use git_config::File;
    use git_testtools::fixture_path_standalone;

    #[test]
    fn parse_config_with_windows_line_endings_successfully() {
        File::from_path_no_includes(&fixture_path_standalone("repo-config.crlf"), git_config::Source::Local).unwrap();
    }
}

mod access;
mod impls;
mod init;
mod mutable;
mod resolve_includes;
mod write;
