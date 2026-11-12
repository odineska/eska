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
    let mut request = Vec::new();
    input.get_ref().set_read_timeout(Some(LIMIT))?;
    input.read_until(b'\n', &mut request)?;
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
