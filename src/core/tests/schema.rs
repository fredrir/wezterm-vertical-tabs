use std::{fs, process::Command};

#[test]
fn generated_schema_checks_accept_windows_line_endings_and_reject_stale_content() {
    let root = std::env::temp_dir().join(format!("vtabs-schema-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let binary = env!("CARGO_BIN_EXE_gen-schema");
    for format in ["lua", "types", "markdown"] {
        let output = Command::new(binary).arg(format).output().unwrap();
        assert!(output.status.success());
        let generated = String::from_utf8(output.stdout).unwrap();
        let path = root.join(format);
        fs::write(&path, generated.replace('\n', "\r\n")).unwrap();
        let check = Command::new(binary)
            .args(["--check", format])
            .arg(&path)
            .output()
            .unwrap();
        assert!(
            check.status.success(),
            "{}",
            String::from_utf8_lossy(&check.stderr)
        );

        fs::write(&path, format!("{generated}stale content\r\n")).unwrap();
        let check = Command::new(binary)
            .args(["--check", format])
            .arg(&path)
            .output()
            .unwrap();
        assert!(!check.status.success());
        assert!(String::from_utf8_lossy(&check.stderr).contains("is stale"));
    }
    fs::remove_dir_all(root).unwrap();
}
