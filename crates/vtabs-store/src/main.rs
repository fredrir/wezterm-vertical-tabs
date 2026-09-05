use std::{
    io::{self, Read, Write},
    path::PathBuf,
};
use vtabs_store::{ErrorCode, MAX_REQUEST_BYTES, Request, Response, StoreError};

fn run() -> Response {
    let mut arguments = std::env::args_os().skip(1);
    let path = match (arguments.next(), arguments.next(), arguments.next()) {
        (Some(flag), Some(path), None) if flag == "--db" => PathBuf::from(path),
        _ => {
            return Response::failure(
                0,
                StoreError::new(
                    ErrorCode::InvalidRequest,
                    "usage: wez-vtabs-store --db PATH",
                ),
            );
        }
    };
    let mut input = Vec::new();
    if let Err(error) = io::stdin()
        .take((MAX_REQUEST_BYTES + 1) as u64)
        .read_to_end(&mut input)
    {
        return Response::failure(
            0,
            StoreError::new(ErrorCode::InvalidRequest, error.to_string()),
        );
    }
    if input.len() > MAX_REQUEST_BYTES {
        return Response::failure(0, StoreError::new(ErrorCode::Limit, "request too large"));
    }
    let request: Request = match serde_json::from_slice(&input) {
        Ok(request) => request,
        Err(error) => {
            return Response::failure(
                0,
                StoreError::new(ErrorCode::InvalidRequest, error.to_string()),
            );
        }
    };
    let result = vtabs_store::sqlite::open(&path)
        .and_then(|mut connection| vtabs_store::sqlite::execute(&mut connection, &request));
    match result {
        Ok(response) => response,
        Err(error) => Response::failure(request.request_id, error),
    }
}

fn main() {
    let response = run();
    let stdout = io::stdout();
    let mut output = stdout.lock();
    if serde_json::to_writer(&mut output, &response).is_err() || writeln!(output).is_err() {
        std::process::exit(2);
    }
    if response.error.is_some() {
        std::process::exit(1);
    }
}
