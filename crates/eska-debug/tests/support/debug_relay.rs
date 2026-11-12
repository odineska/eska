use std::{
    io::{self, BufRead, BufReader, Write},
    net::{Shutdown, SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

const LIMIT: Duration = Duration::from_secs(8);

pub struct Relay {
    address: SocketAddr,
    armed: Arc<AtomicBool>,
    stopped: Arc<AtomicBool>,
    stalled: mpsc::Receiver<()>,
    closed: mpsc::Receiver<()>,
}

impl Relay {
    pub fn start(endpoint: SocketAddr) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let armed = Arc::new(AtomicBool::new(false));
        let stopped = Arc::new(AtomicBool::new(false));
        let (stall, stalled) = mpsc::channel();
        let (close, closed) = mpsc::channel();
        let (inject, stop) = (Arc::clone(&armed), Arc::clone(&stopped));
        thread::spawn(move || {
            for client in listener.incoming() {
                if stop.load(Ordering::Acquire) {
                    break;
                }
                let (inject, stall, close) = (Arc::clone(&inject), stall.clone(), close.clone());
                thread::spawn(move || {
                    let _ = forward(&client?, endpoint, &inject, &stall, &close);
                    Ok::<(), io::Error>(())
                });
            }
        });
        Self {
            address,
            armed,
            stopped,
            stalled,
            closed,
        }
    }

    pub const fn address(&self) -> SocketAddr {
        self.address
    }

    pub fn arm(&self) {
        assert!(!self.armed.swap(true, Ordering::AcqRel));
    }

    pub fn await_stall(&self) {
        self.stalled
            .recv_timeout(LIMIT)
            .expect("evaluation reached the relay");
    }

    pub fn await_close(&self) {
        self.closed
            .recv_timeout(LIMIT)
            .expect("adapter closed the stalled socket");
    }
}

impl Drop for Relay {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        let _ = TcpStream::connect(self.address);
    }
}

fn forward(
    client: &TcpStream,
    endpoint: SocketAddr,
    armed: &AtomicBool,
    stalled: &mpsc::Sender<()>,
    closed: &mpsc::Sender<()>,
) -> io::Result<()> {
    let mut input = BufReader::new(client.try_clone()?);
    input.get_ref().set_read_timeout(Some(LIMIT))?;
    let request = read_request(&mut input)?;
    if request.starts_with(b"POST /e1crdbg/rdbg?cmd=evalExpr ")
        && armed.swap(false, Ordering::AcqRel)
    {
        let _ = stalled.send(());
        io::copy(&mut input, &mut io::sink())?;
        let _ = closed.send(());
        return Ok(());
    }

    let mut upstream = TcpStream::connect_timeout(&endpoint, LIMIT)?;
    upstream.set_read_timeout(Some(LIMIT))?;
    upstream.set_write_timeout(Some(LIMIT))?;
    upstream.write_all(&request)?;
    let mut response = upstream.try_clone()?;
    let mut output = client.try_clone()?;
    output.set_write_timeout(Some(LIMIT))?;
    thread::scope(|scope| {
        scope.spawn(move || {
            let _ = io::copy(&mut input, &mut upstream);
            let _ = upstream.shutdown(Shutdown::Write);
        });
        let result = io::copy(&mut response, &mut output);
        let _ = client.shutdown(Shutdown::Both);
        result.map(|_| ())
    })
}

fn read_request(input: &mut impl BufRead) -> io::Result<Vec<u8>> {
    let mut request = Vec::new();
    let mut body_len = 0;
    loop {
        let offset = request.len();
        if input.read_until(b'\n', &mut request)? == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        let line = &request[offset..];
        if line == b"\r\n" {
            break;
        }
        if let Some((name, value)) = std::str::from_utf8(line)
            .ok()
            .and_then(|line| line.split_once(':'))
            && name.eq_ignore_ascii_case("Content-Length")
        {
            body_len = value.trim().parse().map_err(io::Error::other)?;
        }
    }
    let header_len = request.len();
    request.resize(header_len + body_len, 0);
    input.read_exact(&mut request[header_len..])?;
    Ok(request)
}
