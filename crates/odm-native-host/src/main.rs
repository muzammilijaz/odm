//! Chrome/Edge Native Messaging host for the ODM extension.
//!
//! Chrome launches this process and talks to it over stdio using the Native
//! Messaging framing: each message is a 4-byte native-byte-order length
//! prefix followed by that many bytes of UTF-8 JSON — in both directions.
//! Every message here is just relayed to the desktop app's local loopback
//! HTTP API (see `desktop/src-tauri/src/http_api.rs`).

// Chrome launches this as a child process and talks to it purely over
// stdio -- there's no console I/O to show, so suppress the console window
// Windows would otherwise flash open for a plain console-subsystem exe.
#![windows_subsystem = "windows"]

use serde_json::{json, Value};
use std::io::{self, Read, Write};

/// Must match `http_api::DEFAULT_PORT` in the desktop app.
const LOCAL_API_BASE: &str = "http://127.0.0.1:38019";

fn main() {
    let client = reqwest::blocking::Client::new();
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut stdin = stdin.lock();
    let mut stdout = stdout.lock();

    loop {
        let message = match read_message(&mut stdin) {
            Ok(Some(msg)) => msg,
            Ok(None) => break, // stdin closed — Chrome ended the connection
            Err(_) => break,
        };

        let response = handle_message(&client, &message);
        if write_message(&mut stdout, &response).is_err() {
            break;
        }
    }
}

fn read_message<R: Read>(reader: &mut R) -> io::Result<Option<Value>> {
    let mut len_buf = [0u8; 4];
    if let Err(e) = reader.read_exact(&mut len_buf) {
        if e.kind() == io::ErrorKind::UnexpectedEof {
            return Ok(None);
        }
        return Err(e);
    }
    let len = u32::from_ne_bytes(len_buf) as usize;

    // Native Messaging caps individual messages at 1 MiB from the browser
    // side; refuse anything larger rather than allocating unbounded memory.
    if len > 1024 * 1024 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "message too large"));
    }

    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf)?;
    let value: Value = serde_json::from_slice(&buf).unwrap_or(Value::Null);
    Ok(Some(value))
}

fn write_message<W: Write>(writer: &mut W, value: &Value) -> io::Result<()> {
    let bytes = serde_json::to_vec(value)?;
    let len = (bytes.len() as u32).to_ne_bytes();
    writer.write_all(&len)?;
    writer.write_all(&bytes)?;
    writer.flush()
}

fn handle_message(client: &reqwest::blocking::Client, message: &Value) -> Value {
    match message.get("action").and_then(Value::as_str) {
        Some("add_download") => add_download(client, message),
        Some("ping") => ping(client),
        Some(other) => json!({ "ok": false, "error": format!("unknown action: {other}") }),
        None => json!({ "ok": false, "error": "missing 'action' field" }),
    }
}

/// Lets the extension popup show a live "app running?" indicator without
/// actually queuing a download -- just checks the local HTTP API responds.
fn ping(client: &reqwest::blocking::Client) -> Value {
    match client.get(format!("{LOCAL_API_BASE}/api/downloads")).send() {
        Ok(resp) if resp.status().is_success() => json!({ "ok": true }),
        Ok(resp) => json!({ "ok": false, "error": format!("unexpected status: {}", resp.status()) }),
        Err(e) => json!({ "ok": false, "error": format!("could not reach the ODM app — is it running? ({e})") }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn round_trips_a_message_through_the_framing() {
        let mut buf: Vec<u8> = Vec::new();
        let sent = json!({ "action": "add_download", "url": "https://example.com/f.zip" });
        write_message(&mut buf, &sent).unwrap();

        let mut cursor = Cursor::new(buf);
        let received = read_message(&mut cursor).unwrap().unwrap();
        assert_eq!(received, sent);

        // A second read on the same (now-exhausted) cursor hits EOF cleanly.
        assert!(read_message(&mut cursor).unwrap().is_none());
    }

    #[test]
    fn handle_message_rejects_unknown_action() {
        let client = reqwest::blocking::Client::new();
        let msg = json!({ "action": "not_a_real_action" });
        let resp = handle_message(&client, &msg);
        assert_eq!(resp["ok"], false);
    }

    #[test]
    fn handle_message_requires_url_for_add_download() {
        let client = reqwest::blocking::Client::new();
        let msg = json!({ "action": "add_download" });
        let resp = handle_message(&client, &msg);
        assert_eq!(resp["ok"], false);
    }
}

fn add_download(client: &reqwest::blocking::Client, message: &Value) -> Value {
    let Some(url) = message.get("url").and_then(Value::as_str) else {
        return json!({ "ok": false, "error": "missing 'url' field" });
    };
    let filename = message.get("filename").and_then(Value::as_str);

    let body = json!({ "url": url, "filename": filename });
    let result = client
        .post(format!("{LOCAL_API_BASE}/api/downloads"))
        .json(&body)
        .send();

    match result {
        Ok(resp) if resp.status().is_success() => {
            let task = resp.json::<Value>().unwrap_or(Value::Null);
            json!({ "ok": true, "task": task })
        }
        Ok(resp) => {
            let status = resp.status();
            let text = resp.text().unwrap_or_default();
            json!({ "ok": false, "error": format!("ODM rejected the download ({status}): {text}") })
        }
        Err(e) => json!({
            "ok": false,
            "error": format!("could not reach the ODM app — is it running? ({e})")
        }),
    }
}
