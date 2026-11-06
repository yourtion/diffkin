// One prompt turn on a fresh ACP v1 stdio session. The client advertises no host tools.
use super::{invalid, RepositoryError};
use serde_json::{json, Value};
use std::path::Path;

pub(super) struct Turn<'a> {
    cwd: &'a Path,
    prompt: &'a str,
    pending_id: u8,
    session: Option<String>,
    text: String,
}

fn enqueue(out: &mut Vec<u8>, message: Value) {
    out.extend_from_slice(message.to_string().as_bytes());
    out.push(b'\n');
}

impl<'a> Turn<'a> {
    pub(super) fn start(cwd: &'a Path, prompt: &'a str, out: &mut Vec<u8>) -> Self {
        let turn = Self {
            cwd,
            prompt,
            pending_id: 1,
            session: None,
            text: String::new(),
        };
        turn.request(
            out,
            "initialize",
            json!({"protocolVersion":1,"clientCapabilities":{}, "clientInfo":{"name":"diffkin","version":env!("CARGO_PKG_VERSION")}}),
        );
        turn
    }

    fn request(&self, out: &mut Vec<u8>, method: &str, params: Value) {
        enqueue(
            out,
            json!({"jsonrpc":"2.0","id":self.pending_id,"method":method,"params":params}),
        );
    }

    pub(super) fn receive(
        &mut self,
        line: &[u8],
        out: &mut Vec<u8>,
    ) -> Result<Option<String>, RepositoryError> {
        let malformed = || invalid("ACP 工具返回了无效的协议消息");
        let msg: Value = serde_json::from_slice(line).map_err(|_| malformed())?;
        if msg["jsonrpc"] != "2.0" {
            return Err(malformed());
        }
        if let Some(method) = msg["method"].as_str() {
            if let Some(id) = msg.get("id") {
                let reply = if method == "session/request_permission" {
                    json!({"jsonrpc":"2.0","id":id,"result":{"outcome":{"outcome":"cancelled"}}})
                } else {
                    json!({"jsonrpc":"2.0","id":id,"error":{"code":-32601,"message":"Unsupported client method"}})
                };
                enqueue(out, reply);
            } else if method == "session/update"
                && self.pending_id == 3
                && msg["params"]["sessionId"].as_str() == self.session.as_deref()
            {
                let update = &msg["params"]["update"];
                if update["sessionUpdate"] == "agent_message_chunk"
                    && update["content"]["type"] == "text"
                {
                    self.text
                        .push_str(update["content"]["text"].as_str().ok_or_else(malformed)?);
                }
            }
            return Ok(None);
        }
        if msg["id"].as_u64() != Some(self.pending_id.into()) {
            return Err(malformed());
        }
        if msg.get("error").is_some() {
            let error = &msg["error"];
            if error["code"] == -32000
                || error["data"]
                    .to_string()
                    .to_ascii_lowercase()
                    .contains("secret not found")
            {
                return Err(invalid("ACP 工具缺少登录凭据，请先在终端登录该工具后重试"));
            }
            let stage = match self.pending_id {
                1 => "初始化",
                2 => "建立会话",
                _ => "生成",
            };
            let code = msg["error"]["code"].as_i64().ok_or_else(malformed)?;
            return Err(invalid(format!(
                "ACP 工具在{stage}时返回 {code}，请在终端确认登录和默认模型"
            )));
        }
        let result = &msg["result"];
        match self.pending_id {
            1 if result["protocolVersion"] == 1 => {
                self.pending_id = 2;
                self.request(out, "session/new", json!({"cwd":self.cwd,"mcpServers":[]}));
            }
            2 => {
                let session = result["sessionId"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .ok_or_else(malformed)?;
                self.pending_id = 3;
                self.session = Some(session.into());
                self.request(
                    out,
                    "session/prompt",
                    json!({"sessionId":session,"prompt":[{"type":"text","text":self.prompt}]}),
                );
            }
            3 if result["stopReason"] == "end_turn" => {
                return Ok(Some(std::mem::take(&mut self.text)))
            }
            3 => return Err(invalid("ACP 工具提前结束了本次生成")),
            _ => return Err(malformed()),
        }
        Ok(None)
    }
}
