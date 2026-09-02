use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use vtabs_engine::settings::normalize::{NormalizeRequest, normalize};
use vtabs_runtime::log::Logger;

const NORMALIZE_REQUEST_MAX: u64 = 1024 * 1024;

fn main() {
    // `frame` renders the Zen background and exits; it never touches the terminal or the protocol.
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("frame") => {
            if let Err(err) = vtabs_zen::frame::run(args) {
                eprintln!("{err}");
                std::process::exit(2);
            }
            return;
        }
        Some("settings") => {
            if let Err(err) = settings(args) {
                eprintln!("settings normalize: {err}");
                std::process::exit(2);
            }
            return;
        }
        _ => {}
    }
    if let Err(err) = vtabs_runtime::run() {
        Logger::from_env().log(format!("exit: {err}"));
        std::process::exit(1);
    }
}

fn settings(mut args: impl Iterator<Item = String>) -> Result<(), String> {
    if args.next().as_deref() != Some("normalize") || args.next().as_deref() != Some("--input") {
        return Err("usage: wez-vtabs settings normalize --input PATH".into());
    }
    let path = PathBuf::from(
        args.next()
            .ok_or_else(|| "usage: wez-vtabs settings normalize --input PATH".to_owned())?,
    );
    if args.next().is_some() {
        return Err("usage: wez-vtabs settings normalize --input PATH".into());
    }
    let body = read_private_bounded(&path)?;
    let response = normalize_request(&body)?;
    let mut stdout = std::io::stdout().lock();
    stdout
        .write_all(&response)
        .and_then(|_| stdout.write_all(b"\n"))
        .map_err(|_| "cannot write response".to_owned())
}

fn normalize_request(body: &str) -> Result<Vec<u8>, String> {
    let request: NormalizeRequest =
        serde_json::from_str(body).map_err(|_| "invalid request".to_owned())?;
    let response = normalize(request).map_err(str::to_owned)?;
    serde_json::to_vec(&response).map_err(|_| "cannot encode response".to_owned())
}

fn read_private_bounded(path: &Path) -> Result<String, String> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options
        .open(path)
        .map_err(|_| "cannot open request".to_owned())?;
    let metadata = file
        .metadata()
        .map_err(|_| "cannot inspect request".to_owned())?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("request is not a regular private file".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err("request permissions are not private".into());
        }
    }
    if metadata.len() > NORMALIZE_REQUEST_MAX {
        return Err("request is too large".into());
    }
    let mut bytes = Vec::with_capacity(usize::try_from(metadata.len()).unwrap_or(0));
    file.take(NORMALIZE_REQUEST_MAX + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "cannot read request".to_owned())?;
    if bytes.len() as u64 > NORMALIZE_REQUEST_MAX {
        return Err("request is too large".into());
    }
    String::from_utf8(bytes).map_err(|_| "request is not UTF-8".into())
}

#[cfg(test)]
mod tests {
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
            "normalizer_v": vtabs_engine::settings::normalize::NORMALIZER_VERSION,
            "plugin_version": env!("CARGO_PKG_VERSION"),
            "schema_id": vtabs_engine::settings::schema::identity(),
            "persisted": "{\"version\":1,\"options\":{\"width\":31}}",
            "opts": {"width": 42},
            "explicit": [["width"]],
        });
        let response = normalize_request(&request.to_string()).unwrap();
        let response: serde_json::Value = serde_json::from_slice(&response).unwrap();
        assert_eq!(response["values"]["width"], 42.0);
        assert_eq!(response["warnings"], serde_json::json!([]));
    }
}
