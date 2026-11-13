//! DAP stdio framing and request correlation.
use std::io::{self, BufRead, Read, Write};

use serde::Deserialize;
use serde_json::{Value, json};

const MAX_PACKET: usize = 2 * 1024 * 1024;
const MAX_HEADER: u64 = 1024;

#[derive(Debug, Deserialize)]
pub struct Request {
    #[serde(rename = "type")]
    message_type: String,
    pub seq: u64,
    pub command: String,
    #[serde(default)]
    pub arguments: Value,
}

pub fn read(stream: &mut impl BufRead) -> io::Result<Option<Request>> {
    let mut length = None;
    for index in 0..16 {
        let mut line = String::new();
        if Read::take(&mut *stream, MAX_HEADER + 1).read_line(&mut line)? == 0 {
            return if index == 0 { Ok(None) } else { Err(invalid()) };
        }
        if line.len() as u64 > MAX_HEADER || !line.ends_with("\r\n") {
            return Err(invalid());
        }
        if line == "\r\n" {
            let mut body = vec![0; length.ok_or_else(invalid)?];
            stream.read_exact(&mut body)?;
            let request: Request = serde_json::from_slice(&body).map_err(|_| invalid())?;
            return if request.message_type == "request" {
                Ok(Some(request))
            } else {
                Err(invalid())
            };
        }
        let (name, value) = line.split_once(':').ok_or_else(invalid)?;
        if name.eq_ignore_ascii_case("content-length") {
            if length.is_some() {
                return Err(invalid());
            }
            length = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .ok()
                    .filter(|len| (1..=MAX_PACKET).contains(len))
                    .ok_or_else(invalid)?,
            );
        }
    }
    Err(invalid())
}

pub struct Output<W> {
    stream: W,
    seq: u64,
}

impl<W: Write> Output<W> {
    pub const fn new(stream: W) -> Self {
        Self { stream, seq: 0 }
    }

    pub fn response(&mut self, request: &Request, result: Result<Value, String>) -> io::Result<()> {
        let mut response = json!({
            "type": "response", "command": request.command, "request_seq": request.seq,
            "success": result.is_ok(),
        });
        match result {
            Ok(body) => response["body"] = body,
            Err(message) => response["message"] = json!(message),
        }
        self.send(response)
    }

    pub fn event(&mut self, event: &str, body: &Value) -> io::Result<()> {
        self.send(json!({"type":"event", "event":event, "body":body}))
    }

    fn send(&mut self, mut value: Value) -> io::Result<()> {
        self.seq += 1;
        value["seq"] = json!(self.seq);
        let body = serde_json::to_vec(&value).map_err(|_| invalid())?;
        if body.len() > MAX_PACKET {
            return Err(invalid());
        }
        write!(self.stream, "Content-Length: {}\r\n\r\n", body.len())?;
        self.stream.write_all(&body)?;
        self.stream.flush()
    }
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid DAP packet")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufReader, Cursor};

    #[test]
    fn reads_fragmented_and_coalesced_requests_and_rejects_duplicate_length() {
        let packet = r#"{"seq":1,"type":"request","command":"initialize"}"#;
        let frame = format!("Content-Length: {}\r\n\r\n{packet}", packet.len());
        let mut reader = BufReader::with_capacity(3, Cursor::new(format!("{frame}{frame}")));
        assert_eq!(read(&mut reader).unwrap().unwrap().command, "initialize");
        assert_eq!(read(&mut reader).unwrap().unwrap().seq, 1);
        assert!(read(&mut reader).unwrap().is_none());
        let bad = b"Content-Length: 2\r\nContent-Length: 2\r\n\r\n{}";
        assert!(read(&mut &bad[..]).is_err());
    }
}
