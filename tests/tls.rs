use assert_cmd::{Command, cargo};
use predicates::str::contains;
use reqwest::StatusCode;
use reqwest::blocking::Client;
use reqwest::tls::{Identity, Version};
use rstest::rstest;
use select::{document::Document, node::Node};

mod fixtures;

use crate::fixtures::{Error, FILES, TestServer, reqwest_client, reqwest_client_builder, server};

/// Can start the server with TLS and receive encrypted responses.
#[rstest]
#[case(server(&[
        "--tls-cert", "tests/data/cert_rsa.pem",
        "--tls-key", "tests/data/key_pkcs8.pem",
]))]
#[case(server(&[
        "--tls-cert", "tests/data/cert_rsa.pem",
        "--tls-key", "tests/data/key_pkcs1.pem",
]))]
#[case(server(&[
        "--tls-cert", "tests/data/cert_ec.pem",
        "--tls-key", "tests/data/key_ec.pem",
]))]
fn tls_works(#[case] server: TestServer, reqwest_client: Client) -> Result<(), Error> {
    let body = reqwest_client
        .get(server.url())
        .send()?
        .error_for_status()?;
    let parsed = Document::from_read(body)?;
    for &file in FILES {
        assert!(parsed.find(|x: &Node| x.text() == file).next().is_some());
    }

    Ok(())
}

/// TLS 1.2 is refused, even when rustls is built with support for it like in tests.
#[rstest]
fn tls_1_2_is_refused(
    #[with(&["--tls-cert", "tests/data/cert_ec.pem", "--tls-key", "tests/data/key_ec.pem"])]
    server: TestServer,
) -> Result<(), Error> {
    let client = reqwest_client_builder()
        .tls_version_max(Version::TLS_1_2)
        .build()?;
    assert!(client.get(server.url()).send().is_err());

    Ok(())
}

/// Arguments for a server that requires certificates issued by the test client CA
const CLIENT_CA_ARGS: &[&str] = &[
    "--tls-cert",
    "tests/data/cert_ec.pem",
    "--tls-key",
    "tests/data/key_ec.pem",
    "--tls-client-ca",
    "tests/data/client_ca.pem",
];

/// Only clients with a certificate issued by the CA from --tls-client-ca are let in.
#[rstest]
#[case::trusted_cert(server(CLIENT_CA_ARGS), Some("tests/data/client.pem"), true)]
#[case::untrusted_cert(server(CLIENT_CA_ARGS), Some("tests/data/client_untrusted.pem"), false)]
#[case::no_cert(server(CLIENT_CA_ARGS), None, false)]
fn tls_client_auth(
    #[case] server: TestServer,
    #[case] identity: Option<&str>,
    #[case] accepted: bool,
) -> Result<(), Error> {
    let mut client = reqwest_client_builder();
    if let Some(identity) = identity {
        client = client.identity(Identity::from_pem(&std::fs::read(identity)?)?);
    }
    let response = client.build()?.get(server.url()).send();
    let status = response.ok().map(|res| res.status());
    assert_eq!(status, accepted.then_some(StatusCode::OK));

    Ok(())
}

/// Unusable client CA files are reported.
#[rstest]
#[case::missing(&["--tls-client-ca", "wrong"], "Error: Couldn't access TLS client CA \"wrong\"")]
#[case::without_certificate(
    &["--tls-client-ca", "tests/data/key_ec.pem"],
    "Error: No certificate found in TLS client CA \"tests/data/key_ec.pem\""
)]
fn unusable_tls_client_auth_file(#[case] args: &[&str], #[case] error: &str) -> Result<(), Error> {
    Command::new(cargo::cargo_bin!("miniserve"))
        .args([
            "--tls-cert",
            "tests/data/cert_ec.pem",
            "--tls-key",
            "tests/data/key_ec.pem",
        ])
        .args(args)
        .assert()
        .failure()
        .stderr(contains(error));

    Ok(())
}

/// Wrong path for cert throws error.
#[rstest]
fn wrong_path_cert() -> Result<(), Error> {
    Command::new(cargo::cargo_bin!("miniserve"))
        .args(["--tls-cert", "wrong", "--tls-key", "tests/data/key.pem"])
        .assert()
        .failure()
        .stderr(contains("Error: Couldn't access TLS certificate \"wrong\""));

    Ok(())
}

/// Wrong paths for key throws errors.
#[rstest]
fn wrong_path_key() -> Result<(), Error> {
    Command::new(cargo::cargo_bin!("miniserve"))
        .args(["--tls-cert", "tests/data/cert.pem", "--tls-key", "wrong"])
        .assert()
        .failure()
        .stderr(contains("Error: Couldn't access TLS key \"wrong\""));

    Ok(())
}
