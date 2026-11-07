// One JSON request on stdin produces one response after owned browser resources have closed.
mod worker;
mod worker_protocol;
use std::io::{Read, Write};
use worker_protocol::{MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES, PROTOCOL_VERSION};

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments == ["--version"] {
        println!("n2-kraken-worker {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    let response = if arguments.is_empty() {
        process()
    } else {
        failure(
            None,
            "invalid_arguments",
            "Expected one JSON request on stdin",
        )
    };
    let mut success = response["success"] == true;
    let output = match serde_json::to_vec(&response) {
        Ok(bytes) if bytes.len() <= MAX_RESPONSE_BYTES => bytes,
        _ => {
            success = false;
            serde_json::to_vec(&failure(
                response["id"].as_str(),
                "output_limit",
                "Native response exceeds its byte limit",
            ))
            .expect("Static bounded error response")
        }
    };
    let mut stdout = std::io::stdout().lock();
    if stdout
        .write_all(&output)
        .and_then(|_| stdout.write_all(b"\n"))
        .and_then(|_| stdout.flush())
        .is_err()
    {
        std::process::exit(1);
    }
    std::process::exit(if success { 0 } else { 2 });
}

fn process() -> serde_json::Value {
    let mut bytes = Vec::new();
    if std::io::stdin()
        .take((MAX_REQUEST_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return failure(None, "input_error", "Native request could not be read");
    }
    let request = match worker_protocol::read_request(&bytes) {
        Ok(request) => request,
        Err(message) => return failure(None, "invalid_request", message),
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(8)
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            return failure(
                Some(&request.id),
                "runtime_error",
                "Native runtime could not start",
            );
        }
    };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        runtime.block_on(worker::execute(request.command))
    }));
    runtime.block_on(n2_kraken::browser::shutdown());
    runtime.shutdown_timeout(std::time::Duration::from_secs(1));
    match result {
        Ok(Ok(data)) => {
            serde_json::json!({"protocol":PROTOCOL_VERSION,"id":request.id,"success":true,"data":data})
        }
        Ok(Err(message)) => failure(Some(&request.id), "operation_failed", &message),
        Err(_) => failure(
            Some(&request.id),
            "operation_panicked",
            "Native operation stopped unexpectedly",
        ),
    }
}

fn failure(id: Option<&str>, code: &str, message: &str) -> serde_json::Value {
    serde_json::json!({"protocol":PROTOCOL_VERSION,"id":id,"success":false,
        "error":{"code":code,"message":message.chars().take(4096).collect::<String>()}})
}
