use serde_json::Value;

#[cfg(windows)]
const PIPE_NAME: &str = r"\\.\pipe\NidavellirCore";

pub async fn call_service_with_params(
    method: &str,
    params: Option<Value>,
) -> Result<Value, String> {
    let request = match params {
        Some(params) => serde_json::json!({ "method": method, "params": params }),
        None => serde_json::json!({ "method": method }),
    };
    #[cfg(windows)]
    {
        let response = exchange(
            PIPE_NAME,
            &format!("{request}\n"),
            std::time::Duration::from_secs(5),
            std::time::Duration::from_secs(30),
        )
        .await?;
        parse_response(&response)
    }
    #[cfg(not(windows))]
    {
        let _ = request;
        Err("Nidavellir Core Service IPC requires Windows".into())
    }
}

fn parse_response(response: &str) -> Result<Value, String> {
    let parsed: Value = serde_json::from_str(response)
        .map_err(|e| format!("Invalid service response; action outcome unknown: {e}"))?;
    if parsed.get("ok").and_then(|v| v.as_bool()) != Some(true) {
        return Err(parsed
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("Invalid or failed service response")
            .to_string());
    }
    Ok(parsed)
}

/// One connection and one send. Dropping Tokio's pipe cancels owned pending I/O;
/// a timeout does not cancel or retry an operation already accepted by the service.
#[cfg(windows)]
async fn exchange(
    pipe_name: &str,
    request: &str,
    connect_timeout: std::time::Duration,
    response_timeout: std::time::Duration,
) -> Result<String, String> {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
    use tokio::net::windows::named_pipe::ClientOptions;
    use tokio::time::{sleep, timeout};
    use windows::Win32::Foundation::ERROR_PIPE_BUSY;

    let mut pipe = timeout(connect_timeout, async {
        loop {
            match ClientOptions::new().open(pipe_name) {
                Ok(pipe) => return Ok(pipe),
                Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY.0 as i32) => {
                    sleep(std::time::Duration::from_millis(50)).await;
                }
                Err(e) => return Err(format!("Core Service unavailable: {e}")),
            }
        }
    })
    .await
    .map_err(|_| {
        "Core Service is busy; connection timed out before sending the request".to_string()
    })??;

    let response = timeout(response_timeout, async {
        pipe.write_all(request.as_bytes()).await?;
        // Bound both time and memory if a broken peer never terminates its JSON line.
        const MAX_RESPONSE_BYTES: u64 = 16 * 1024 * 1024;
        let mut reader = BufReader::new(pipe.take(MAX_RESPONSE_BYTES));
        let mut response = String::new();
        reader.read_line(&mut response).await?;
        if !response.ends_with('\n') {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData, "incomplete or oversized response",
            ));
        }
        Ok(response)
    }).await
        .map_err(|_| "Core Service response timed out; action outcome unknown. Wait for status to reconnect before another action. The request was not retried.".to_string())?;
    response.map_err(|e: std::io::Error| format!("Core Service connection lost; action outcome unknown. Wait for status to reconnect: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_and_failed_envelopes_never_become_success() {
        for response in ["not json", "{}", r#"{"ok":false,"error":"stock refused"}"#] {
            assert!(parse_response(response).is_err());
        }
        assert!(parse_response(r#"{"ok":true,"data":{"type":"Pong"}}"#).is_ok());
    }

    #[cfg(windows)]
    mod pipes {
        use super::*;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::time::Duration;
        use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
        use tokio::net::windows::named_pipe::ServerOptions;
        use tokio::time::timeout;

        static NEXT_PIPE: AtomicUsize = AtomicUsize::new(0);
        fn name() -> String {
            format!(
                r"\\.\pipe\nidavellir-ipc-test-{}-{}",
                std::process::id(),
                NEXT_PIPE.fetch_add(1, Ordering::Relaxed)
            )
        }
        fn run(future: impl std::future::Future<Output = ()>) {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(future);
        }

        #[test]
        fn fragmented_reply_and_partial_reply_are_handled() {
            run(async {
                for complete in [true, false] {
                    let name = name();
                    let mut server = ServerOptions::new()
                        .first_pipe_instance(true)
                        .create(&name)
                        .unwrap();
                    let peer = tokio::spawn(async move {
                        server.connect().await.unwrap();
                        let mut request = String::new();
                        BufReader::new(&mut server)
                            .read_line(&mut request)
                            .await
                            .unwrap();
                        assert_eq!(request, "{\"method\":\"Ping\"}\n");
                        server.write_all(b"{\"ok\":true}").await.unwrap();
                        if complete {
                            tokio::time::sleep(Duration::from_millis(10)).await;
                            server.write_all(b"\n").await.unwrap();
                        }
                        let _ = server.read(&mut [0u8; 1]).await;
                    });
                    let result = exchange(
                        &name,
                        "{\"method\":\"Ping\"}\n",
                        Duration::from_secs(1),
                        Duration::from_millis(150),
                    )
                    .await;
                    if complete {
                        assert_eq!(result.unwrap(), "{\"ok\":true}\n");
                    } else {
                        assert!(result.unwrap_err().contains("outcome unknown"));
                    }
                    timeout(Duration::from_secs(2), peer)
                        .await
                        .unwrap()
                        .unwrap();
                }
            });
        }

        #[test]
        fn unresponsive_peer_times_out_without_retry_and_releases_pipe() {
            run(async {
                let name = name();
                let server = ServerOptions::new()
                    .first_pipe_instance(true)
                    .create(&name)
                    .unwrap();
                let peer = tokio::spawn(async move {
                    server.connect().await.unwrap();
                    let mut reader = BufReader::new(server);
                    let mut request = String::new();
                    reader.read_line(&mut request).await.unwrap();
                    assert_eq!(request, "ResetGpuTuning\n");
                    // EOF/broken pipe proves the deadline dropped the connection, with no resend.
                    match reader.read_line(&mut String::new()).await {
                        Ok(n) => assert_eq!(n, 0),
                        Err(e) => assert!(matches!(
                            e.kind(),
                            std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::UnexpectedEof
                        )),
                    }
                });
                let error = exchange(
                    &name,
                    "ResetGpuTuning\n",
                    Duration::from_secs(1),
                    Duration::from_millis(100),
                )
                .await
                .unwrap_err();
                assert!(error.contains("timed out; action outcome unknown"));
                timeout(Duration::from_secs(2), peer)
                    .await
                    .unwrap()
                    .unwrap();
                ServerOptions::new()
                    .first_pipe_instance(true)
                    .create(&name)
                    .unwrap();
            });
        }

        #[test]
        fn blocked_write_and_busy_connection_have_deadlines() {
            run(async {
                let name = name();
                let server = ServerOptions::new()
                    .first_pipe_instance(true)
                    .max_instances(1)
                    .in_buffer_size(1024)
                    .create(&name)
                    .unwrap();
                let held = tokio::net::windows::named_pipe::ClientOptions::new()
                    .open(&name)
                    .unwrap();
                server.connect().await.unwrap();
                let error = exchange(
                    &name,
                    "Ping\n",
                    Duration::from_millis(80),
                    Duration::from_secs(1),
                )
                .await
                .unwrap_err();
                assert!(error.contains("before sending"));
                drop(held);
                server.disconnect().unwrap();
                let peer = tokio::spawn(async move {
                    server.connect().await.unwrap();
                    std::future::pending::<()>().await;
                });
                let error = exchange(
                    &name,
                    &"x".repeat(4 * 1024 * 1024),
                    Duration::from_secs(1),
                    Duration::from_millis(100),
                )
                .await
                .unwrap_err();
                peer.abort();
                assert!(error.contains("outcome unknown"), "{error}");
            });
        }
    }
}
