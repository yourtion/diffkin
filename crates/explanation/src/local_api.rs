use super::{invalid, parse_output, ModelTask};
use diffkin_domain::{ExplanationProvider, LocalApiConnection, RepositoryError};
use reqwest::{Client, Url};
use serde_json::{json, Value};
use std::{
    net::IpAddr,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

fn endpoint(base: &str) -> Result<Url, RepositoryError> {
    let bad = || invalid("请填写本机 HTTP API 基础地址，例如 http://127.0.0.1:11434/v1");
    let mut url = Url::parse(base.trim()).map_err(|_| bad())?;
    let local = url.host_str().is_some_and(|host| {
        host == "localhost"
            || host
                .trim_matches(['[', ']'])
                .parse::<IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    });
    if base.len() > 2048
        || !local
        || url.scheme() != "http"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(bad());
    }
    url.path_segments_mut()
        .map_err(|_| bad())?
        .pop_if_empty()
        .push("chat")
        .push("completions");
    Ok(url)
}

pub(super) fn generate(
    connection: &LocalApiConnection,
    model: &str,
    task: ModelTask,
    cancel: &AtomicBool,
) -> Result<Value, RepositoryError> {
    let url = endpoint(&connection.base_url)?;
    if model.trim().is_empty() {
        return Err(invalid("请填写本机 API 的模型名称"));
    }
    let key = connection.api_key.as_deref().unwrap_or("");
    if key.len() > 4096 || !key.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(invalid("API 密钥格式无效"));
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| invalid("无法启动本机 API 连接"))?;
    runtime.block_on(async {
        let call = request(url, model, key, task);
        tokio::pin!(call);
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(RepositoryError::Cancelled {
                    message: "AI 请求已取消".into(),
                });
            }
            if let Ok(result) = tokio::time::timeout(Duration::from_millis(25), call.as_mut()).await
            {
                return result;
            }
        }
    })
}

async fn request(
    url: Url,
    model: &str,
    key: &str,
    task: ModelTask,
) -> Result<Value, RepositoryError> {
    let io = || RepositoryError::IoFailure {
        message: "无法连接本机 API，请确认服务已启动并检查地址".into(),
    };
    let timeout = || RepositoryError::BudgetExceeded {
        message: "本机 API 请求超时".into(),
    };
    let transport = |e: reqwest::Error| if e.is_timeout() { timeout() } else { io() };
    let client = Client::builder()
        .no_proxy()
        .resolve_to_addrs(
            "localhost",
            &[
                std::net::SocketAddr::from(([127, 0, 0, 1], 0)),
                std::net::SocketAddr::from((std::net::Ipv6Addr::LOCALHOST, 0)),
            ],
        )
        .redirect(reqwest::redirect::Policy::none())
        .timeout(task.timeout)
        .build()
        .map_err(transport)?;
    let mut request = client.post(url).json(&json!({
        "model": model,
        "stream": false,
        "messages": [
            {"role": "system", "content": format!("{}\n只返回 JSON。输出 schema：{}", task.instructions, task.schema)},
            {"role": "user", "content": task.input.to_string()}
        ]
    }));
    if !key.is_empty() {
        request = request.bearer_auth(key);
    }
    let mut response = request.send().await.map_err(transport)?;
    if !response.status().is_success() {
        return Err(invalid(format!(
            "本机 API 返回 HTTP {}，请检查地址、模型和密钥",
            response.status().as_u16()
        )));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(transport)? {
        if bytes.len() + chunk.len() > 512 * 1024 {
            return Err(RepositoryError::BudgetExceeded {
                message: "本机 API 响应超过 512 KiB".into(),
            });
        }
        bytes.extend_from_slice(&chunk);
    }
    let malformed = || invalid("本机 API 未返回包含文本的 Chat Completions 响应");
    let result: Value = serde_json::from_slice(&bytes).map_err(|_| malformed())?;
    let choice = &result["choices"][0];
    if choice["finish_reason"]
        .as_str()
        .is_some_and(|reason| reason != "stop")
    {
        return Err(invalid("模型未完整返回结果，请检查模型的输出长度设置"));
    }
    parse_output(
        ExplanationProvider::Openai,
        choice["message"]["content"]
            .as_str()
            .ok_or_else(malformed)?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader, Read, Write},
        net::TcpListener,
    };

    #[test]
    fn accepts_only_loopback_base_urls_without_credentials() {
        for base in [
            "http://127.0.0.1:11434/v1/",
            "http://[::1]/v1",
            "http://localhost/v1",
        ] {
            assert_eq!(endpoint(base).unwrap().path(), "/v1/chat/completions");
        }
        for base in [
            "http://example.com/v1",
            "http://localhost.example.com",
            "http://user:pw@localhost",
            "http://127.0.0.1/?key=secret",
            "http://localhost#test",
            "file:///tmp/f",
        ] {
            assert!(endpoint(base).is_err());
        }
    }

    #[test]
    fn cancelling_drops_a_pending_http_call() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let cancel = AtomicBool::new(false);
        std::thread::scope(|threads| {
            let signal = &cancel;
            threads.spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut request = [0; 8192];
                assert!(stream.read(&mut request).unwrap() > 0);
                signal.store(true, Ordering::Relaxed);
                while stream.read(&mut request).unwrap_or(0) > 0 {}
            });
            let result = generate(
                &LocalApiConnection {
                    base_url: url,
                    api_key: None,
                },
                "local",
                ModelTask {
                    instructions: "system",
                    schema: json!({}),
                    input: json!({}),
                    timeout: Duration::from_secs(4),
                },
                &cancel,
            );
            assert!(matches!(result, Err(RepositoryError::Cancelled { .. })));
        });
    }

    #[test]
    fn chat_completions_sends_optional_bearer_and_bounds_error_disclosure() {
        for (key, status) in [(Some("test-key"), 200), (None, 401)] {
            let socket = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = socket.local_addr().unwrap();
            let server = std::thread::spawn(move || {
                let (mut stream, _) = socket.accept().unwrap();
                let mut reader = BufReader::new(&stream);
                let mut header = String::new();
                loop {
                    let mut line = String::new();
                    assert!(reader.read_line(&mut line).unwrap() > 0);
                    if line == "\r\n" {
                        break;
                    }
                    header.push_str(&line.to_lowercase());
                }
                assert!(header.starts_with("post /v1/chat/completions "));
                assert_eq!(
                    header.contains("authorization: bearer test-key"),
                    key.is_some()
                );
                let size: usize = header
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length: "))
                    .unwrap()
                    .parse()
                    .unwrap();
                let mut body = vec![0; size];
                reader.read_exact(&mut body).unwrap();
                let data: Value = serde_json::from_slice(&body).unwrap();
                assert_eq!(data["model"], "test-model");
                assert_eq!(data["messages"][1]["content"], r#"{"frozen":true}"#);
                let body = if status == 200 {
                    json!({"choices":[{"finish_reason":"stop","message":{"content":"{\"verified\":true}"}}]}).to_string()
                } else {
                    "private response detail".into()
                };
                write!(stream,"HTTP/1.1 {status} Status\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
            });
            let connection = LocalApiConnection {
                base_url: format!("http://{address}/v1"),
                api_key: key.map(String::from),
            };
            let result = generate(
                &connection,
                "test-model",
                ModelTask {
                    instructions: "system",
                    schema: json!({}),
                    input: json!({"frozen":true}),
                    timeout: Duration::from_secs(3),
                },
                &AtomicBool::new(false),
            );
            server.join().unwrap();
            if status == 200 {
                assert_eq!(result.unwrap(), json!({"verified":true}));
            } else {
                let error = result.unwrap_err().to_string();
                assert!(error.contains("401") && !error.contains("private response detail"));
            }
        }
    }
}
