use onenote_index::protocol::{Response, ResponseEnvelope};
use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn independent_process_negotiates_lists_and_shuts_down() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let mut child = Command::new(env!("CARGO_BIN_EXE_onenote-query"))
        .args([
            "--database",
            temporary
                .path()
                .join("protocol.sqlite")
                .to_str()
                .expect("UTF-8 test path"),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn protocol adapter");
    {
        let stdin = child.stdin.as_mut().expect("protocol stdin");
        writeln!(
            stdin,
            r#"{{"protocol_version":1,"request_id":"hello","operation":"hello","supported_versions":[1]}}"#
        )
        .expect("write hello");
        writeln!(
            stdin,
            r#"{{"protocol_version":1,"request_id":"sources","operation":"list_sources"}}"#
        )
        .expect("write sources");
        writeln!(
            stdin,
            r#"{{"protocol_version":1,"request_id":"bye","operation":"shutdown"}}"#
        )
        .expect("write shutdown");
    }
    let output = child.wait_with_output().expect("protocol output");
    assert!(output.status.success(), "{:?}", output.stderr);
    assert!(output.stderr.is_empty());
    let responses: Vec<ResponseEnvelope> = String::from_utf8(output.stdout)
        .expect("UTF-8 protocol")
        .lines()
        .map(|line| serde_json::from_str(line).expect("response JSON"))
        .collect();
    assert_eq!(responses.len(), 3);
    assert!(matches!(responses[0].response, Response::Hello { .. }));
    assert!(matches!(
        responses[1].response,
        Response::Sources { ref sources } if sources.is_empty()
    ));
    assert!(matches!(responses[2].response, Response::Goodbye));
}
