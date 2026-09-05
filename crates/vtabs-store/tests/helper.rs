#![cfg(feature = "sqlite")]

use std::io::Write;
use std::process::{Command, Stdio};
use vtabs_store::{ErrorCode, Response};

fn invoke(input: &[u8], database: &std::path::Path) -> (bool, Response) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_wez-vtabs-store"))
        .arg("--db")
        .arg(database)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    let output = child.wait_with_output().unwrap();
    (
        output.status.success(),
        serde_json::from_slice(&output.stdout).unwrap(),
    )
}

#[test]
fn helper_returns_bounded_versioned_errors_over_the_actual_pipe() {
    let database = std::env::temp_dir().join(format!("vtabs-helper-{}.sqlite", std::process::id()));
    let (success, response) = invoke(
        br#"{"version":99,"request_id":42,"operations":[]}"#,
        &database,
    );
    assert!(!success);
    assert_eq!(response.request_id, 42);
    assert_eq!(response.error.unwrap().code, ErrorCode::InvalidRequest);
    let (success, response) = invoke(&vec![b' '; vtabs_store::MAX_REQUEST_BYTES + 1], &database);
    assert!(!success);
    assert_eq!(response.error.unwrap().code, ErrorCode::Limit);
    let (success, response) = invoke(
        br#"{"version":1,"request_id":43,"operations":[]}"#,
        &database,
    );
    assert!(success);
    assert_eq!(response.version, 1);
    assert_eq!(response.request_id, 43);
    assert_eq!(response.revision, 0);
    drop(std::fs::remove_file(database));
}
