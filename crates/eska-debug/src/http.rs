//! Bounded, direct HTTP/1.1 transport for the plain HTTP debugger endpoint.
use std::{
    io::{self, BufRead, BufReader, Read, Write},
    net::{SocketAddr, TcpStream, ToSocketAddrs},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

const MAX_HEADER: usize = 16 * 1024;
const MAX_BODY: usize = 4 * 1024 * 1024;

pub struct Endpoint {
    addresses: Vec<SocketAddr>,
    authority: String,
    measure: bool,
}

impl Endpoint {
    pub fn resolve(url: &url::Url) -> io::Result<Self> {
        let host = url.host_str().ok_or_else(invalid)?;
        let port = url.port_or_known_default().ok_or_else(invalid)?;
        let name = host.trim_matches(['[', ']']).to_owned();
        let (send, receive) = mpsc::sync_channel(1);
        thread::Builder::new().spawn(move || {
            let resolved = (name.as_str(), port)
                .to_socket_addrs()
                .map(|addresses| addresses.take(16).collect());
            let _ = send.send(resolved);
        })?;
        let addresses = receive
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))??;
        Ok(Self {
            addresses,
            authority: format!("{host}:{port}"),
            measure: std::env::var_os("ESKA_DEBUG_TIMINGS").is_some(),
        })
    }

    pub fn post(
        &self,
        resource: &str,
        query: &str,
        xml: &str,
        timeout: Duration,
    ) -> io::Result<Vec<u8>> {
        let started = Instant::now();
        let result = self.exchange(resource, query, xml, started + timeout);
        if self.measure {
            let operation = query
                .strip_prefix("cmd=")
                .and_then(|value| value.split('&').next())
                .unwrap_or_default();
            let duration = started.elapsed().as_secs_f64() * 1000.0;
            match &result {
                Ok(_) => eprintln!("dbgs_http {operation} {duration:.3}"),
                Err(error) => eprintln!("dbgs_http_error {operation} {duration:.3} {error}"),
            }
        }
        result
    }

    fn exchange(
        &self,
        resource: &str,
        query: &str,
        xml: &str,
        deadline: Instant,
    ) -> io::Result<Vec<u8>> {
        if xml.len() > MAX_BODY {
            return Err(invalid());
        }
        let mut connection = None;
        for address in &self.addresses {
            let remaining = remaining(deadline)?;
            if let Ok(stream) =
                TcpStream::connect_timeout(address, remaining.min(Duration::from_millis(500)))
            {
                connection = Some(stream);
                break;
            }
        }
        let stream = connection.ok_or_else(|| io::Error::from(io::ErrorKind::ConnectionRefused))?;
        let mut stream = Deadline { stream, deadline };
        write!(
            stream,
            "POST /e1crdbg/{resource}?{query} HTTP/1.1\r\nHost: {}\r\nUser-Agent: 1CV8\r\nAccept: application/xml, text/xml\r\nContent-Type: application/xml; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            self.authority,
            xml.len()
        )?;
        stream.write_all(xml.as_bytes())?;
        read_response(&mut BufReader::new(stream))
    }
}

struct Deadline {
    stream: TcpStream,
    deadline: Instant,
}

impl Read for Deadline {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.stream
            .set_read_timeout(Some(remaining(self.deadline)?))?;
        self.stream.read(buf)
    }
}

impl Write for Deadline {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.stream
            .set_write_timeout(Some(remaining(self.deadline)?))?;
        self.stream.write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}

fn remaining(deadline: Instant) -> io::Result<Duration> {
    let duration = deadline.saturating_duration_since(Instant::now());
    if duration.is_zero() {
        Err(io::ErrorKind::TimedOut.into())
    } else {
        Ok(duration)
    }
}

fn read_response(reader: &mut impl BufRead) -> io::Result<Vec<u8>> {
    let mut head = Vec::new();
    loop {
        let remaining = MAX_HEADER.saturating_sub(head.len());
        if reader.take(remaining as u64).read_until(b'\n', &mut head)? == 0 {
            return Err(invalid());
        }
        if head.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    let mut headers = [httparse::EMPTY_HEADER; 32];
    let mut response = httparse::Response::new(&mut headers);
    if !matches!(response.parse(&head), Ok(httparse::Status::Complete(_))) {
        return Err(invalid());
    }
    let status = response.code.ok_or_else(invalid)?;
    if !(200..300).contains(&status) {
        return Err(io::Error::other(format!("HTTP {status}")));
    }
    if status == 204 {
        return Ok(Vec::new());
    }
    let mut length = None;
    let mut chunked = false;
    for header in response.headers {
        if header.name.eq_ignore_ascii_case("Content-Length") {
            if length.is_some() {
                return Err(invalid());
            }
            length = Some(
                ascii(header.value)?
                    .trim()
                    .parse::<usize>()
                    .map_err(|_| invalid())?,
            );
        } else if header.name.eq_ignore_ascii_case("Transfer-Encoding") {
            if chunked || !ascii(header.value)?.trim().eq_ignore_ascii_case("chunked") {
                return Err(invalid());
            }
            chunked = true;
        }
    }
    match (chunked, length) {
        (true, None) => read_chunks(reader),
        (false, Some(length)) if length <= MAX_BODY => {
            let mut body = vec![0; length];
            reader.read_exact(&mut body)?;
            Ok(body)
        }
        (false, None) => {
            let mut body = Vec::new();
            reader.take((MAX_BODY + 1) as u64).read_to_end(&mut body)?;
            if body.len() > MAX_BODY {
                Err(invalid())
            } else {
                Ok(body)
            }
        }
        _ => Err(invalid()),
    }
}

fn read_chunks(reader: &mut impl BufRead) -> io::Result<Vec<u8>> {
    let mut body = Vec::new();
    loop {
        let mut line = Vec::new();
        reader.take(128).read_until(b'\n', &mut line)?;
        let line = ascii(&line)?.strip_suffix("\r\n").ok_or_else(invalid)?;
        let size = line.split(';').next().ok_or_else(invalid)?;
        let size = usize::from_str_radix(size, 16).map_err(|_| invalid())?;
        if size == 0 {
            return Ok(body);
        }
        let end = body
            .len()
            .checked_add(size)
            .filter(|len| *len <= MAX_BODY)
            .ok_or_else(invalid)?;
        let start = body.len();
        body.resize(end, 0);
        reader.read_exact(&mut body[start..])?;
        let mut ending = [0; 2];
        reader.read_exact(&mut ending)?;
        if ending != *b"\r\n" {
            return Err(invalid());
        }
    }
}

fn ascii(bytes: &[u8]) -> io::Result<&str> {
    std::str::from_utf8(bytes).map_err(|_| invalid())
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid debugger HTTP response")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_chunked_xml_and_rejects_conflicting_framing() {
        let response =
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n0\r\n\r\n";
        assert_eq!(read_response(&mut &response[..]).unwrap(), b"abc");
        let response =
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Length: 3\r\n\r\n";
        assert!(read_response(&mut &response[..]).is_err());
    }
}
