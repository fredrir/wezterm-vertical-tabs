use super::*;

#[test]
fn bounded_reader_refuses_oversize_and_symlink_inputs() {
    let dir = std::env::temp_dir().join(format!("wez-vtabs-normalize-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let request = dir.join("request.json");
    std::fs::write(&request, vec![b'x'; NORMALIZE_REQUEST_MAX as usize + 1]).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&request, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    assert_eq!(
        read_private_bounded(&request).unwrap_err(),
        "request is too large"
    );
    std::fs::write(&request, "{}").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&request, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            read_private_bounded(&request).unwrap_err(),
            "request permissions are not private"
        );
        std::fs::set_permissions(&request, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    assert_eq!(read_private_bounded(&request).unwrap(), "{}");
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let link = dir.join("request-link.json");
        let _ = std::fs::remove_file(&link);
        symlink(&request, &link).unwrap();
        assert_eq!(
            read_private_bounded(&link).unwrap_err(),
            "cannot open request"
        );
        std::fs::remove_file(link).unwrap();
    }
    std::fs::remove_file(request).unwrap();
    std::fs::remove_dir(dir).unwrap();
}

#[test]
fn normalize_request_resolves_persistence_before_explicit_options() {
    let request = serde_json::json!({
        "plugin_version": env!("CARGO_PKG_VERSION"),
        "schema_id": vtabs_engine::settings::schema::identity(),
        "persisted": "{\"options\":{\"width\":31}}",
        "opts": {"width": 42},
        "explicit": [["width"]],
    });
    let response = normalize_request(&request.to_string()).unwrap();
    let response: serde_json::Value = serde_json::from_slice(&response).unwrap();
    assert_eq!(response["values"]["width"], 42.0);
    assert_eq!(response["warnings"], serde_json::json!([]));
}
