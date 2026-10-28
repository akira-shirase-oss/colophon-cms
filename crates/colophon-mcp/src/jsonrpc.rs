//! JSON-RPC 2.0 as MCP's stdio transport uses it (MCP 2025-11-25, "Base protocol" and
//! "Transports"): one UTF-8 JSON object per line, no batches (removed in 2025-06-18), request ids
//! that are strings or integers and never `null`.
//!
//! [`parse`] turns one line into a request, a notification or a client's response, or into the
//! error response the line deserves. [`LineReader`] splits the input into lines with a size limit,
//! so no message can make the server hold more than [`MAX_MESSAGE_BYTES`].

use serde_json::{Map, Value, json};
use tokio::io::{AsyncBufReadExt as _, AsyncRead, BufReader};

/// The largest message read: the API's request body limit is 1 MiB, and a tool call carries one
/// request body plus a small envelope.
pub const MAX_MESSAGE_BYTES: usize = 2 * 1024 * 1024;

/// Invalid JSON.
pub const PARSE_ERROR: i64 = -32_700;
/// Not a valid request object.
pub const INVALID_REQUEST: i64 = -32_600;
/// No such method.
pub const METHOD_NOT_FOUND: i64 = -32_601;
/// Invalid parameters, including an unknown tool (MCP "Tools", "Error handling").
pub const INVALID_PARAMS: i64 = -32_602;
/// A failure inside this server.
pub const INTERNAL_ERROR: i64 = -32_603;
/// The Colophon API refused or couldn't answer a request this server needed to answer the
/// method (from the range JSON-RPC reserves for implementation-defined server errors).
pub const API_UNAVAILABLE: i64 = -32_001;
/// A request other than `initialize` or `ping` before the session was initialized.
pub const NOT_INITIALIZED: i64 = -32_002;

/// A request id, as the client sent it: a string or an integer.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RequestId(Value);

impl RequestId {
    /// The id, if `value` is a valid one.
    pub fn new(value: &Value) -> Option<Self> {
        match value {
            Value::String(_) => Some(Self(value.clone())),
            Value::Number(number) if number.is_i64() || number.is_u64() => {
                Some(Self(value.clone()))
            }
            _ => None,
        }
    }

    /// The id as JSON, to echo back exactly.
    pub fn as_value(&self) -> &Value {
        &self.0
    }
}

/// A request: it gets exactly one response.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    /// Its id.
    pub id: RequestId,
    /// The method.
    pub method: String,
    /// The parameters: an object, empty when none were sent.
    pub params: Map<String, Value>,
}

/// A notification: it never gets a response.
#[derive(Debug, Clone, PartialEq)]
pub struct Notification {
    /// The method.
    pub method: String,
    /// The parameters: an object, empty when none were sent.
    pub params: Map<String, Value>,
}

/// One message from the client.
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    /// A request.
    Request(Request),
    /// A notification.
    Notification(Notification),
    /// A response to a request the server sent. This server sends none, so it is ignored.
    Response,
}

/// Parses one line. An `Err` holds the error response to send (with the id when it could be
/// read, `null` otherwise), or `None` for a broken notification, which gets no response.
pub fn parse(line: &[u8]) -> Result<Incoming, Option<String>> {
    let value: Value = serde_json::from_slice(line)
        .map_err(|_| Some(error(None, PARSE_ERROR, "parse error: not valid JSON")))?;
    let Value::Object(mut message) = value else {
        let what = if value.is_array() {
            "batches are not supported (MCP 2025-11-25)"
        } else {
            "a message is a JSON object"
        };
        return Err(Some(error(None, INVALID_REQUEST, what)));
    };
    // `Some(None)`: an id is there but isn't a string or an integer.
    let id = message.get("id").map(RequestId::new);
    let known = id.clone().flatten();
    let invalid = |why: &str| Some(error(known.as_ref(), INVALID_REQUEST, why));
    if message.get("jsonrpc") != Some(&json!("2.0")) {
        return Err(invalid("jsonrpc must be \"2.0\""));
    }
    let Some(method) = message.remove("method") else {
        if message.contains_key("result") || message.contains_key("error") {
            return Ok(Incoming::Response);
        }
        return Err(invalid("method is required"));
    };
    let Value::String(method) = method else {
        return Err(invalid("method must be a string"));
    };
    let id = match id {
        None => None,
        Some(Some(id)) => Some(id),
        Some(None) => {
            return Err(invalid("id must be a string or an integer, never null"));
        }
    };
    let params = match message.remove("params") {
        None => Map::new(),
        Some(Value::Object(params)) => params,
        Some(_) => {
            return Err(id.map(|id| error(Some(&id), INVALID_PARAMS, "params must be an object")));
        }
    };
    Ok(match id {
        Some(id) => Incoming::Request(Request { id, method, params }),
        None => Incoming::Notification(Notification { method, params }),
    })
}

/// A success response, serialized on one line.
pub fn success(id: &RequestId, result: &Value) -> String {
    json!({"jsonrpc": "2.0", "id": id.as_value(), "result": result}).to_string()
}

/// An error response, serialized on one line; the id is `null` when it couldn't be read.
pub fn error(id: Option<&RequestId>, code: i64, message: &str) -> String {
    error_with(id, code, message, None)
}

/// An error response with `data`.
pub fn error_with(id: Option<&RequestId>, code: i64, message: &str, data: Option<Value>) -> String {
    let mut error = json!({"code": code, "message": message});
    if let Some(data) = data {
        error["data"] = data;
    }
    json!({
        "jsonrpc": "2.0",
        "id": id.map_or(&Value::Null, RequestId::as_value),
        "error": error,
    })
    .to_string()
}

/// A line read from the input.
#[derive(Debug, PartialEq, Eq)]
pub enum Line {
    /// A message's bytes, without the newline (or a trailing `\r`).
    Message(Vec<u8>),
    /// A line longer than the limit; its bytes were dropped as they arrived.
    TooLong,
}

/// Splits input into lines of at most `max` bytes, without ever holding more than that.
///
/// Cancel-safe: the only await is `fill_buf`, which consumes nothing, and the partial line lives
/// in this struct, so dropping [`LineReader::next_line`] loses nothing.
#[derive(Debug)]
pub struct LineReader<R> {
    inner: BufReader<R>,
    line: Vec<u8>,
    overflowed: bool,
    max: usize,
}

impl<R: AsyncRead + Unpin> LineReader<R> {
    /// Reads `input` with lines of at most `max` bytes.
    pub fn new(input: R, max: usize) -> Self {
        Self {
            inner: BufReader::new(input),
            line: Vec::new(),
            overflowed: false,
            max,
        }
    }

    /// The next non-blank line, or `None` at the end of the input.
    pub async fn next_line(&mut self) -> std::io::Result<Option<Line>> {
        loop {
            let available = self.inner.fill_buf().await?;
            if available.is_empty() {
                return Ok(self.finish());
            }
            let newline = available.iter().position(|&byte| byte == b'\n');
            let part = &available[..newline.unwrap_or(available.len())];
            if !self.overflowed {
                if self.line.len() + part.len() > self.max {
                    self.overflowed = true;
                    self.line = Vec::new();
                } else {
                    self.line.extend_from_slice(part);
                }
            }
            let used = part.len() + usize::from(newline.is_some());
            self.inner.consume(used);
            if newline.is_some()
                && let Some(line) = self.finish()
            {
                return Ok(Some(line));
            }
        }
    }

    /// Ends the current line: what it was, or `None` if it was blank.
    fn finish(&mut self) -> Option<Line> {
        if std::mem::take(&mut self.overflowed) {
            return Some(Line::TooLong);
        }
        let mut line = std::mem::take(&mut self.line);
        if line.last() == Some(&b'\r') {
            line.pop();
        }
        (!line.iter().all(u8::is_ascii_whitespace)).then_some(Line::Message(line))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refused(line: &str) -> Value {
        let text = parse(line.as_bytes())
            .expect_err("refused")
            .expect("a response");
        assert!(!text.contains('\n'), "one line: {text}");
        serde_json::from_str(&text).expect("JSON")
    }

    #[test]
    fn malformed_json_is_a_parse_error_with_a_null_id() {
        let response = refused("{\"jsonrpc\": \"2.0\", \"id\": 1, \"method\": ");
        assert_eq!(response["error"]["code"], PARSE_ERROR);
        assert_eq!(response["id"], Value::Null);
        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(refused("\u{fffd}not json")["error"]["code"], PARSE_ERROR);
    }

    #[test]
    fn batches_and_non_objects_are_invalid_requests() {
        let batch = refused(r#"[{"jsonrpc":"2.0","id":1,"method":"ping"}]"#);
        assert_eq!(batch["error"]["code"], INVALID_REQUEST);
        assert!(
            batch["error"]["message"]
                .as_str()
                .is_some_and(|text| text.contains("batch"))
        );
        assert_eq!(refused("42")["error"]["code"], INVALID_REQUEST);
        assert_eq!(refused("[]")["error"]["code"], INVALID_REQUEST);
    }

    #[test]
    fn a_request_needs_version_method_and_a_usable_id() {
        let no_version = refused(r#"{"id":7,"method":"ping"}"#);
        assert_eq!(no_version["error"]["code"], INVALID_REQUEST);
        assert_eq!(no_version["id"], 7, "the id is echoed when it can be read");
        let old_version = refused(r#"{"jsonrpc":"1.0","id":"a","method":"ping"}"#);
        assert_eq!(old_version["id"], "a");
        let no_method = refused(r#"{"jsonrpc":"2.0","id":8}"#);
        assert_eq!(no_method["error"]["code"], INVALID_REQUEST);
        assert_eq!(no_method["id"], 8);
        for id in ["null", "1.5", "true", "{}"] {
            let response = refused(&format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"ping"}}"#));
            assert_eq!(response["error"]["code"], INVALID_REQUEST, "{id}");
            assert_eq!(response["id"], Value::Null, "{id}");
        }
        let numeric_method = refused(r#"{"jsonrpc":"2.0","id":9,"method":5}"#);
        assert_eq!(numeric_method["error"]["code"], INVALID_REQUEST);
    }

    #[test]
    fn params_must_be_an_object() {
        let response = refused(r#"{"jsonrpc":"2.0","id":3,"method":"tools/list","params":[1]}"#);
        assert_eq!(response["error"]["code"], INVALID_PARAMS);
        assert_eq!(response["id"], 3);
        assert_eq!(
            parse(br#"{"jsonrpc":"2.0","method":"notifications/x","params":1}"#),
            Err(None),
            "a broken notification gets no response"
        );
    }

    #[test]
    fn requests_notifications_and_responses_are_told_apart() {
        let Ok(Incoming::Request(request)) =
            parse(br#"{"jsonrpc":"2.0","id":"r-1","method":"tools/list"}"#)
        else {
            panic!("a request");
        };
        assert_eq!(request.id.as_value(), "r-1");
        assert_eq!(request.method, "tools/list");
        assert!(request.params.is_empty());
        let Ok(Incoming::Notification(notification)) = parse(
            br#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":1}}"#,
        ) else {
            panic!("a notification");
        };
        assert_eq!(notification.params["requestId"], 1);
        assert_eq!(
            parse(br#"{"jsonrpc":"2.0","id":4,"result":{}}"#),
            Ok(Incoming::Response)
        );
    }

    #[test]
    fn responses_echo_the_id_on_one_line() {
        let id = RequestId::new(&json!(12)).expect("an id");
        let text = success(&id, &json!({"text": "two\nlines"}));
        assert!(!text.contains('\n'));
        let value: Value = serde_json::from_str(&text).expect("JSON");
        assert_eq!(
            value,
            json!({"jsonrpc": "2.0", "id": 12, "result": {"text": "two\nlines"}})
        );
        let failed: Value =
            serde_json::from_str(&error_with(Some(&id), INTERNAL_ERROR, "x", Some(json!(1))))
                .expect("JSON");
        assert_eq!(
            failed["error"],
            json!({"code": -32603, "message": "x", "data": 1})
        );
    }

    async fn lines(input: &[u8], max: usize) -> Vec<Line> {
        let mut reader = LineReader::new(input, max);
        let mut out = Vec::new();
        while let Some(line) = reader.next_line().await.expect("in memory") {
            out.push(line);
        }
        out
    }

    #[tokio::test]
    async fn lines_split_on_newlines_and_skip_blank_ones() {
        assert_eq!(
            lines(b"one\r\n\n  \ntwo\nthree", 16).await,
            vec![
                Line::Message(b"one".to_vec()),
                Line::Message(b"two".to_vec()),
                Line::Message(b"three".to_vec()),
            ]
        );
    }

    #[tokio::test]
    async fn an_oversized_line_is_dropped_and_the_next_one_still_reads() {
        let mut input = vec![b'x'; 40];
        input.extend_from_slice(b"\nok\n");
        input.extend(vec![b'y'; 40]);
        assert_eq!(
            lines(&input, 16).await,
            vec![Line::TooLong, Line::Message(b"ok".to_vec()), Line::TooLong]
        );
    }

    #[tokio::test]
    async fn a_line_split_across_reads_is_reassembled() {
        let (mut writer, reader) = tokio::io::duplex(4);
        let task = tokio::spawn(async move {
            use tokio::io::AsyncWriteExt as _;
            writer
                .write_all(b"{\"jsonrpc\":\"2.0\"}\nnext\n")
                .await
                .expect("write");
        });
        let mut reader = LineReader::new(reader, 64);
        assert_eq!(
            reader.next_line().await.expect("read"),
            Some(Line::Message(b"{\"jsonrpc\":\"2.0\"}".to_vec()))
        );
        assert_eq!(
            reader.next_line().await.expect("read"),
            Some(Line::Message(b"next".to_vec()))
        );
        task.await.expect("writer");
        assert_eq!(reader.next_line().await.expect("read"), None);
    }
}
