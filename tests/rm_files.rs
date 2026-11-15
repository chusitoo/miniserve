mod fixtures;

use assert_fs::fixture::TempDir;
use fixtures::{Error, TestServer, server, tmpdir};
use percent_encoding::utf8_percent_encode;
use reqwest::StatusCode;
use reqwest::blocking::Client;
use rstest::rstest;
use select::document::Document;
use select::predicate::{Attr, Class};
use std::path::{Component, Path};
use url::Url;

use crate::fixtures::{
    DEEPLY_NESTED_FILE, DIRECTORIES, DIRECTORY_SYMLINK, FILE_SYMLINK, FILES, HIDDEN_DIRECTORIES,
    HIDDEN_FILES, reqwest_client,
};

const NESTED_FILES_UNDER_SINGLE_ROOT: &[&str] = &["someDir/alpha", "someDir/some_sub_dir/bravo"];

/// Construct a path for a GET request,
/// with each path component being separately encoded.
fn make_get_path(unencoded_path: impl AsRef<Path>) -> String {
    unencoded_path
        .as_ref()
        .components()
        .map(|comp| match comp {
            Component::Prefix(_) | Component::RootDir => unreachable!("Not currently used"),
            Component::CurDir => ".",
            Component::ParentDir => "..",
            Component::Normal(comp) => comp.to_str().unwrap(),
        })
        .map(|comp| utf8_percent_encode(comp, percent_encoding::NON_ALPHANUMERIC).to_string())
        .collect::<Vec<_>>()
        .join("/")
}

/// Construct a path for a deletion POST request without any further encoding.
///
/// This should be kept consistent with implementation.
fn make_del_path(unencoded_path: impl AsRef<Path>) -> String {
    format!("rm?path=/{}", make_get_path(unencoded_path))
}

/// Tests that deletion requests succeed as expected.
/// Verifies that the path exists, can be deleted, and is no longer accessible after deletion.
fn assert_rm_ok(
    reqwest_client: &Client,
    base_url: Url,
    unencoded_path: impl AsRef<Path>,
) -> Result<(), Error> {
    let file_path = unencoded_path.as_ref();

    // encode
    let get_url = base_url.join(&make_get_path(file_path))?;
    let del_url = base_url.join(&make_del_path(file_path))?;

    // check path exists
    let _get_res = reqwest_client
        .get(get_url.clone())
        .send()?
        .error_for_status()?;

    // delete
    let _del_res = reqwest_client.post(del_url).send()?.error_for_status()?;

    // check path is gone
    let get_res = reqwest_client.get(get_url).send()?;
    if get_res.status() != StatusCode::NOT_FOUND {
        return Err(format!("Unexpected status code: {}", get_res.status()).into());
    }

    Ok(())
}

/// Tests that deletion requests fail as expected.
/// The `check_path_exists` parameter allows skipping this check before and after
/// the deletion attempt in case the path should be inaccessible via GET.
fn assert_rm_err(
    reqwest_client: &Client,
    base_url: Url,
    unencoded_path: impl AsRef<Path>,
    check_path_exists: bool,
) -> Result<(), Error> {
    let file_path = unencoded_path.as_ref();

    // encode
    let get_url = base_url.join(&make_get_path(file_path))?;
    let del_url = base_url.join(&make_del_path(file_path))?;

    // check path exists
    if check_path_exists {
        let _get_res = reqwest_client
            .get(get_url.clone())
            .send()?
            .error_for_status()?;
    }

    // delete
    let del_res = reqwest_client.post(del_url).send()?;
    if !del_res.status().is_client_error() {
        return Err(format!("Unexpected status code: {}", del_res.status()).into());
    }

    // check path still exists
    if check_path_exists {
        let _get_res = reqwest_client.get(get_url).send()?.error_for_status()?;
    }

    Ok(())
}

#[rstest]
#[case(FILES[0])]
#[case(FILES[1])]
#[case(FILES[2])]
#[case(DIRECTORIES[0])]
#[case(DIRECTORIES[1])]
#[case(DIRECTORIES[2])]
#[case(DEEPLY_NESTED_FILE)]
fn rm_disabled_by_default(
    server: TestServer,
    reqwest_client: Client,
    #[case] path: &str,
) -> Result<(), Error> {
    assert_rm_err(&reqwest_client, server.url(), path, true)
}

#[rstest]
#[case(FILES[0])]
#[case(FILES[1])]
#[case(FILES[2])]
#[case(HIDDEN_FILES[0])]
#[case(HIDDEN_FILES[1])]
#[case(DIRECTORIES[0])]
#[case(DIRECTORIES[1])]
#[case(DIRECTORIES[2])]
#[case(HIDDEN_DIRECTORIES[0])]
#[case(HIDDEN_DIRECTORIES[1])]
#[case(DEEPLY_NESTED_FILE)]
fn rm_disabled_by_default_with_hidden(
    reqwest_client: Client,
    #[with(&["-H"])] server: TestServer,
    #[case] path: &str,
) -> Result<(), Error> {
    assert_rm_err(&reqwest_client, server.url(), path, true)
}

#[rstest]
#[case(FILES[0])]
#[case(FILES[1])]
#[case(FILES[2])]
#[case(DIRECTORIES[0])]
#[case(DIRECTORIES[1])]
#[case(DIRECTORIES[2])]
#[case(DEEPLY_NESTED_FILE)]
fn rm_works(
    #[with(&["-R"])] server: TestServer,
    reqwest_client: Client,
    #[case] path: &str,
) -> Result<(), Error> {
    assert_rm_ok(&reqwest_client, server.url(), path)
}

#[rstest]
#[case(server(&["-R"]), "", true)]
#[case(server(&[] as &[&str]), "", false)]
#[case(server(&["-R", "someDir"]), "", false)]
#[case(server(&["-R", "someDir"]), "someDir/", true)]
fn rm_selection_shown_where_rm_allowed(
    #[case] server: TestServer,
    #[case] dir: &str,
    #[case] shown: bool,
    reqwest_client: Client,
) -> Result<(), Error> {
    let body = reqwest_client
        .get(server.url().join(dir)?)
        .send()?
        .error_for_status()?;
    let parsed = Document::from_read(body)?;
    let checkboxes = parsed.find(Class("rm-select")).count();

    assert_eq!(
        parsed.find(Attr("id", "rm-selected")).next().is_some(),
        shown
    );
    assert_eq!(checkboxes, parsed.find(Class("rm_form")).count());
    assert_eq!(checkboxes > 0, shown);

    Ok(())
}

#[rstest]
#[case(HIDDEN_FILES[0])]
#[case(HIDDEN_FILES[1])]
#[case(HIDDEN_DIRECTORIES[0])]
#[case(HIDDEN_DIRECTORIES[1])]
fn cannot_rm_hidden_when_disallowed(
    #[with(&["-R"])] server: TestServer,
    reqwest_client: Client,
    #[case] path: &str,
) -> Result<(), Error> {
    assert_rm_err(&reqwest_client, server.url(), path, false)
}

#[rstest]
#[case(HIDDEN_FILES[0])]
#[case(HIDDEN_FILES[1])]
#[case(HIDDEN_DIRECTORIES[0])]
#[case(HIDDEN_DIRECTORIES[1])]
fn can_rm_hidden_when_allowed(
    #[with(&["-R", "-H"])] server: TestServer,
    reqwest_client: Client,
    #[case] path: &str,
) -> Result<(), Error> {
    assert_rm_ok(&reqwest_client, server.url(), path)
}

/// This test runs the server with --allowed-rm-dir argument and checks that
/// deletions in a different directory are actually prevented.
#[rstest]
#[case(server(&["-R", "someOtherDir"]), NESTED_FILES_UNDER_SINGLE_ROOT[0])]
#[case(server(&["-R", "someOtherDir"]), NESTED_FILES_UNDER_SINGLE_ROOT[1])]
#[case(server(&["-R", "someDir/some_other_sub_dir"]), NESTED_FILES_UNDER_SINGLE_ROOT[0])]
#[case(server(&["-R", "someDir/some_other_sub_dir"]), NESTED_FILES_UNDER_SINGLE_ROOT[1])]
fn rm_is_restricted(
    #[case] server: TestServer,
    reqwest_client: Client,
    #[case] path: &str,
) -> Result<(), Error> {
    assert_rm_err(&reqwest_client, server.url(), path, true)
}

/// This test runs the server with --allowed-rm-dir argument and checks that
/// deletions of the specified directories themselves are allowed.
///
/// Both ways of specifying multiple directories are tested.
#[rstest]
#[case(server(&["-R", "dira,dirb,dir space"]), DIRECTORIES[0])]
#[case(server(&["-R", "dira,dirb,dir space"]), DIRECTORIES[1])]
#[case(server(&["-R", "dira,dirb,dir space"]), DIRECTORIES[2])]
#[case(server(&["-R", "dira", "-R", "dirb", "-R", "dir space"]), DIRECTORIES[0])]
#[case(server(&["-R", "dira", "-R", "dirb", "-R", "dir space"]), DIRECTORIES[1])]
#[case(server(&["-R", "dira", "-R", "dirb", "-R", "dir space"]), DIRECTORIES[2])]
fn can_rm_allowed_dir(
    #[case] server: TestServer,
    reqwest_client: Client,
    #[case] path: &str,
) -> Result<(), Error> {
    assert_rm_ok(&reqwest_client, server.url(), path)
}

/// This tests that we can delete from directories specified by --allow-rm-dir.
#[rstest]
#[case(server(&["-R", "someDir"]), "someDir/alpha")]
#[case(server(&["-R", "someDir"]), "someDir//alpha")]
#[case(server(&["-R", "someDir"]), "someDir/././alpha")]
#[case(server(&["-R", "someDir"]), "someDir/some_sub_dir")]
#[case(server(&["-R", "someDir"]), "someDir/some_sub_dir/")]
#[case(server(&["-R", "someDir"]), "someDir//some_sub_dir")]
#[case(server(&["-R", "someDir"]), "someDir/./some_sub_dir")]
#[case(server(&["-R", "someDir"]), "someDir/some_sub_dir/bravo")]
#[case(server(&["-R", "someDir"]), "someDir//some_sub_dir//bravo")]
#[case(server(&["-R", "someDir"]), "someDir/./some_sub_dir/../some_sub_dir/bravo")]
#[case(server(&["-R", "someDir/some_sub_dir"]), "someDir/some_sub_dir/bravo")]
#[case(server(&["-R", Path::new("someDir/some_sub_dir").to_str().unwrap()]),
    "someDir/some_sub_dir/bravo")]
fn can_rm_from_allowed_dir(
    #[case] server: TestServer,
    reqwest_client: Client,
    #[case] file: &str,
) -> Result<(), Error> {
    assert_rm_ok(&reqwest_client, server.url(), file)
}

/// Test deleting from symlinked directories that point to outside the server root.
#[rstest]
#[case(server(&["-R"]), true)]
#[case(server(&["-R", "--no-symlinks"]), false)]
fn rm_from_symlinked_dir(
    #[case] server: TestServer,
    #[case] should_succeed: bool,
    #[from(tmpdir)] target: TempDir,
    reqwest_client: Client,
) -> Result<(), Error> {
    #[cfg(unix)]
    use std::os::unix::fs::symlink as symlink_dir;
    #[cfg(windows)]
    use std::os::windows::fs::symlink_dir;

    // create symlink
    let link: &Path = Path::new("linked");
    symlink_dir(target.path(), server.path().join(link))?;

    let files_through_link = [FILES, DIRECTORIES]
        .concat()
        .iter()
        .map(|name| link.join(name))
        .collect::<Vec<_>>();
    if should_succeed {
        for file_path in &files_through_link {
            assert_rm_ok(&reqwest_client, server.url(), file_path)?;
        }
    } else {
        for file_path in &files_through_link {
            assert_rm_err(&reqwest_client, server.url(), file_path, false)?;
        }
    }
    Ok(())
}

/// The served directory itself can never be deleted.
#[rstest]
#[case("rm?path=/")]
#[case("rm?path=")]
fn cannot_rm_served_dir(
    #[with(&["-R"])] server: TestServer,
    reqwest_client: Client,
    #[case] del_path: &str,
) -> Result<(), Error> {
    let status = reqwest_client
        .post(server.url().join(del_path)?)
        .send()?
        .status();

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(server.path().join(FILES[0]).exists());
    Ok(())
}

/// Deleting a symlink removes the link itself, never its target.
#[rstest]
#[case(DIRECTORY_SYMLINK, DIRECTORIES[0])]
#[case(FILE_SYMLINK, FILES[0])]
fn rm_symlink_keeps_target(
    #[with(&["-R"])] server: TestServer,
    reqwest_client: Client,
    #[case] link: &str,
    #[case] target: &str,
) -> Result<(), Error> {
    let link_path = server.path().join(link.trim_end_matches('/'));
    assert!(link_path.is_symlink());

    reqwest_client
        .post(server.url().join(&make_del_path(link))?)
        .send()?
        .error_for_status()?;

    assert!(!link_path.is_symlink());
    assert!(server.path().join(target).exists());
    Ok(())
}
