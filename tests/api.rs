use std::collections::HashMap;

use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use reqwest::{StatusCode, blocking::Client};
use rstest::rstest;

mod fixtures;

use crate::fixtures::{DIRECTORIES, Error, TestServer, reqwest_client, server};

/// Test that we can get dir size for plain paths as well as percent-encoded paths
#[rstest]
#[case(DIRECTORIES[0].to_string())]
#[case(DIRECTORIES[1].to_string())]
#[case(DIRECTORIES[2].to_string())]
#[case(utf8_percent_encode(DIRECTORIES[0], NON_ALPHANUMERIC).to_string())]
#[case(utf8_percent_encode(DIRECTORIES[1], NON_ALPHANUMERIC).to_string())]
#[case(utf8_percent_encode(DIRECTORIES[2], NON_ALPHANUMERIC).to_string())]
fn api_dir_size(
    #[case] dir: String,
    #[with(&["--directory-size"])] server: TestServer,
    reqwest_client: Client,
) -> Result<(), Error> {
    let mut command = HashMap::new();
    command.insert("DirSize", dir);

    let resp = reqwest_client
        .post(server.url().join("__miniserve_internal/api")?)
        .json(&command)
        .send()?
        .error_for_status()?;

    assert_eq!(resp.status(), StatusCode::OK);
    assert_ne!(resp.text()?, "0 B");

    Ok(())
}

/// Test for path traversal vulnerability (CWE-22) in DirSize parameter.
#[rstest]
#[case("/tmp")] // Not CWE-22, but `foo` isn't a directory
#[case("/../foo")]
#[case("../foo")]
#[case("../tmp")]
#[case("/tmp")]
#[case("/foo")]
#[case("C:/foo")]
#[case(r"C:\foo")]
#[case(r"\foo")]
fn api_dir_size_prevent_path_transversal_attacks(
    #[case] path: &str,
    #[with(&["--directory-size"])] server: TestServer,
    reqwest_client: Client,
) -> Result<(), Error> {
    let mut command = HashMap::new();
    command.insert("DirSize", path);

    let resp = reqwest_client
        .post(server.url().join("__miniserve_internal/api")?)
        .json(&command)
        .send()?;

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    Ok(())
}

#[rstest]
fn api_requires_auth(
    #[with(&["--directory-size", "--auth", "testuser:testpassword"])] server: TestServer,
    reqwest_client: Client,
) -> Result<(), Error> {
    let mut command = HashMap::new();
    command.insert("DirSize", DIRECTORIES[0]);
    let url = server.url().join("__miniserve_internal/api")?;

    let resp = reqwest_client.post(url.clone()).json(&command).send()?;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    let resp = reqwest_client
        .post(url)
        .basic_auth("testuser", Some("testpassword"))
        .json(&command)
        .send()?;
    assert_eq!(resp.status(), StatusCode::OK);

    Ok(())
}

/// The API route contains the random route, so error pages must not include it
#[rstest]
fn api_route_not_leaked_by_random_route_error_page(
    #[with(&["--directory-size", "--random-route"])] server: TestServer,
    reqwest_client: Client,
) -> Result<(), Error> {
    let resp = reqwest_client.get(server.url()).send()?;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    assert!(!resp.text()?.contains("__miniserve_internal/api"));

    let mut command = HashMap::new();
    command.insert("DirSize", DIRECTORIES[0]);
    let resp = reqwest_client
        .post(server.url().join("__miniserve_internal/api")?)
        .json(&command)
        .send()?;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    Ok(())
}
