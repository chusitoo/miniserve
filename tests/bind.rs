use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

use assert_cmd::{cargo, prelude::*};
use assert_fs::fixture::TempDir;
use regex::Regex;
use reqwest::blocking::Client;
use rstest::rstest;

mod fixtures;

use crate::fixtures::{Error, TestServer, port, reqwest_client, server, tmpdir};

#[rstest]
#[case(&["-i", "12.123.234.12"])]
#[case(&["-i", "::", "-i", "12.123.234.12"])]
fn bind_fails(tmpdir: TempDir, port: u16, #[case] args: &[&str]) -> Result<(), Error> {
    Command::new(cargo::cargo_bin!("miniserve"))
        .arg(tmpdir.path())
        .arg("-p")
        .arg(port.to_string())
        .args(args)
        .assert()
        .stderr(predicates::str::contains("Failed to bind server to"))
        .failure();

    Ok(())
}

#[rstest]
#[case(server(&[] as &[&str]), true, true)]
#[case(server(&["-i", "::"]), false, true)]
#[case(server(&["-i", "0.0.0.0"]), true, false)]
#[case(server(&["-i", "::", "-i", "0.0.0.0"]), true, true)]
fn bind_ipv4_ipv6(
    #[case] server: TestServer,
    reqwest_client: Client,
    #[case] bind_ipv4: bool,
    #[case] bind_ipv6: bool,
) -> Result<(), Error> {
    assert_eq!(
        reqwest_client
            .get(format!("http://127.0.0.1:{}", server.port()).as_str())
            .send()
            .is_ok(),
        bind_ipv4
    );
    assert_eq!(
        reqwest_client
            .get(format!("http://[::1]:{}", server.port()).as_str())
            .send()
            .is_ok(),
        bind_ipv6
    );

    Ok(())
}

#[rstest]
#[case(&[] as &[&str])]
#[case(&["-i", "::"])]
#[case(&["-i", "127.0.0.1"])]
#[case(&["-i", "0.0.0.0"])]
#[case(&["-i", "::", "-i", "0.0.0.0"])]
#[case(&["--random-route"])]
#[case(&["--route-prefix", "/prefix"])]
fn validate_printed_urls(
    reqwest_client: Client,
    tmpdir: TempDir,
    port: u16,
    #[case] args: &[&str],
) -> Result<(), Error> {
    let mut child = Command::new(cargo::cargo_bin!("miniserve"))
        .arg(tmpdir.path())
        .arg("-p")
        .arg(port.to_string())
        .args(args)
        .stdout(Stdio::piped())
        .spawn()?;

    // WARN assumes urls list is terminated by an empty line
    let url_lines = BufReader::new(child.stdout.take().unwrap())
        .lines()
        .map(|line| line.expect("Error reading stdout"))
        .take_while(|line| !line.is_empty()) /* non-empty lines */
        .collect::<Vec<_>>();
    let url_lines = url_lines.join("\n");

    let urls = Regex::new(r"http://[a-zA-Z0-9\.\[\]:/]+")
        .unwrap()
        .captures_iter(url_lines.as_str())
        .map(|caps| caps.get(0).unwrap().as_str())
        .collect::<Vec<_>>();

    assert!(!urls.is_empty());

    for url in urls {
        reqwest_client.get(url).send()?.error_for_status()?;
    }

    child.kill()?;

    Ok(())
}

#[cfg(unix)]
mod unix_socket {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::Path;
    use std::process::{Command, Stdio};
    use std::thread::sleep;
    use std::time::{Duration, Instant};

    use assert_cmd::{cargo, prelude::*};
    use assert_fs::fixture::TempDir;
    use rstest::rstest;

    use crate::fixtures::{Error, FILES, port, tmpdir};

    fn get(socket: &Path, path: &str) -> std::io::Result<String> {
        let mut stream = UnixStream::connect(socket)?;
        write!(stream, "GET {path} HTTP/1.0\r\nHost: localhost\r\n\r\n")?;
        let mut response = String::new();
        stream.read_to_string(&mut response)?;
        Ok(response)
    }

    fn wait_for_socket(socket: &Path) {
        let start_wait = Instant::now();
        while UnixStream::connect(socket).is_err() {
            assert!(
                start_wait.elapsed() < Duration::from_secs(5),
                "timeout waiting for {socket:?}"
            );
            sleep(Duration::from_millis(100));
        }
    }

    #[rstest]
    #[case::new_socket(false)]
    #[case::stale_socket(true)]
    fn serves_only_on_unix_socket(
        tmpdir: TempDir,
        port: u16,
        #[case] stale_socket: bool,
    ) -> Result<(), Error> {
        let socket_dir = TempDir::new()?;
        let socket = socket_dir.path().join("miniserve.sock");
        if stale_socket {
            // Dropping the listener leaves its socket file behind
            drop(UnixListener::bind(&socket)?);
        }

        let mut child = Command::new(cargo::cargo_bin!("miniserve"))
            .arg(tmpdir.path())
            .arg("-p")
            .arg(port.to_string())
            .arg("-i")
            .arg(&socket)
            .stdout(Stdio::null())
            .spawn()?;
        wait_for_socket(&socket);
        let response = get(&socket, &format!("/{}", FILES[0]));
        let tcp = TcpStream::connect(("localhost", port));
        child.kill()?;
        child.wait()?;

        let response = response?;
        assert!(response.starts_with("HTTP/1.0 200"), "{response}");
        assert!(response.ends_with("Test Hello Yes"));
        assert!(tcp.is_err());

        Ok(())
    }

    #[rstest]
    fn keeps_existing_file_at_socket_path(tmpdir: TempDir) -> Result<(), Error> {
        let socket = tmpdir.path().join(FILES[0]);

        Command::new(cargo::cargo_bin!("miniserve"))
            .arg(tmpdir.path())
            .arg("-i")
            .arg(&socket)
            .assert()
            .stderr(predicates::str::contains("File exists and is not a socket"))
            .failure();
        assert_eq!(std::fs::read_to_string(&socket)?, "Test Hello Yes");

        Ok(())
    }

    #[rstest]
    fn refuses_unix_socket_in_use(tmpdir: TempDir) -> Result<(), Error> {
        let socket = tmpdir.path().join("miniserve.sock");
        let _listener = UnixListener::bind(&socket)?;

        Command::new(cargo::cargo_bin!("miniserve"))
            .arg(tmpdir.path())
            .arg("-i")
            .arg(&socket)
            .assert()
            .stderr(predicates::str::contains(
                "Another process is listening on this socket",
            ))
            .failure();

        Ok(())
    }
}
