//! Owned dbgs startup uses the platform's notification file to identify its listener.
use std::{
    ffi::OsString,
    fs::File,
    io::{self, Read as _},
    net::{Ipv4Addr, SocketAddrV4},
    path::Path,
    thread,
    time::{Duration, Instant},
};

use eska_project_core::project::{
    Project,
    local_settings::{DebugEndpoint, Profile, debug_server_url},
};
use url::Url;

use super::{RuntimeError, platform};
use crate::{
    build::{DebugServerChild, Ibcmd},
    native_path,
};

pub enum DebugConnection {
    External(Url),
    Managed { platform: Ibcmd, port: Option<u16> },
}

impl DebugConnection {
    /// Resolve an attach endpoint and discover the runner when the profile owns the server.
    /// # Errors
    /// Returns a missing endpoint or platform error.
    pub fn prepare(project: &Project, profile: &Profile) -> Result<Self, RuntimeError> {
        let debug = profile
            .debug
            .as_ref()
            .ok_or(RuntimeError::EndpointUnavailable)?;
        if debug.managed {
            Ok(Self::Managed {
                platform: platform(project, profile)?,
                port: debug.port,
            })
        } else {
            Self::external(debug)
        }
    }

    pub(super) fn external(debug: &DebugEndpoint) -> Result<Self, RuntimeError> {
        debug
            .url
            .as_deref()
            .and_then(debug_server_url)
            .map(Self::External)
            .ok_or(RuntimeError::EndpointUnavailable)
    }

    /// Obtain the external URL or start and own a loopback dbgs for this session.
    /// # Errors
    /// Returns an I/O error on startup, timeout, or an invalid native notification.
    pub fn open(&self) -> io::Result<(Url, Option<ManagedDebugServer>)> {
        match self {
            Self::External(url) => Ok((url.clone(), None)),
            Self::Managed { platform, port } => {
                let (url, server) = ManagedDebugServer::start(platform, *port)?;
                Ok((url, Some(server)))
            }
        }
    }
}

pub struct ManagedDebugServer {
    child: DebugServerChild,
    _scratch: tempfile::TempDir,
}

impl ManagedDebugServer {
    fn start(platform: &Ibcmd, port: Option<u16>) -> io::Result<(Url, Self)> {
        let scratch = tempfile::Builder::new().prefix("eska-dbgs-").tempdir()?;
        let ready = scratch.path().join("endpoint");
        let mut notify = OsString::from("--notify=");
        notify.push(native_path(&ready).as_os_str());
        let listen = port.map_or_else(
            || "--portRange=49152:65535".to_owned(),
            |port| format!("--port={port}"),
        );
        let child = platform.spawn_managed_debug_server(
            vec!["--addr=127.0.0.1".into(), listen.into()],
            &scratch.path().join("process.pid"),
            notify,
        )?;
        let mut server = Self {
            child,
            _scratch: scratch,
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if server.child.stopped()? {
                return Err(io::Error::other(
                    "dbgs exited before publishing its listener",
                ));
            }
            if let Ok(address) = read_notification(&ready)
                && port.is_none_or(|expected| expected == address.port())
            {
                let url = Url::parse(&format!("http://{address}")).map_err(io::Error::other)?;
                return Ok((url, server));
            }
            thread::sleep(Duration::from_millis(25));
        }
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "dbgs did not publish its listener",
        ))
    }

    /// Check whether the process exited or is no longer observable.
    #[must_use]
    pub fn stopped(&mut self) -> bool {
        self.child.stopped().unwrap_or(true)
    }
}

fn read_notification(path: &Path) -> io::Result<SocketAddrV4> {
    let mut bytes = Vec::new();
    File::open(path)?.take(129).read_to_end(&mut bytes)?;
    parse_notification(&bytes).ok_or_else(|| io::Error::other("invalid dbgs listener"))
}

fn parse_notification(bytes: &[u8]) -> Option<SocketAddrV4> {
    if bytes.len() > 128 {
        return None;
    }
    let address = if let Some(bytes) = bytes.strip_prefix(&[0xff, 0xfe]) {
        let (chars, rest) = bytes.as_chunks::<2>();
        if !rest.is_empty() {
            return None;
        }
        let chars: Vec<_> = chars.iter().copied().map(u16::from_le_bytes).collect();
        String::from_utf16(&chars).ok()?
    } else {
        std::str::from_utf8(bytes)
            .ok()?
            .trim_start_matches('\u{feff}')
            .to_owned()
    };
    let address: SocketAddrV4 = address.trim().parse().ok()?;
    (*address.ip() == Ipv4Addr::LOCALHOST && address.port() != 0).then_some(address)
}

#[cfg(test)]
mod tests {
    use super::parse_notification;

    #[test]
    fn validates_native_notification_before_connecting() {
        let utf16: Vec<_> = [0xff, 0xfe]
            .into_iter()
            .chain("127.0.0.1:49152".encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        assert_eq!(
            parse_notification(&utf16),
            Some("127.0.0.1:49152".parse().unwrap())
        );
        assert_eq!(
            parse_notification(b"127.0.0.1:1550\n"),
            Some("127.0.0.1:1550".parse().unwrap())
        );
        for invalid in [
            b"127.0.0.1:0".as_slice(),
            b"0.0.0.0:1550",
            b"\xff\xfe\x31",
            b"127.0.0.1:1550/path",
        ] {
            assert_eq!(parse_notification(invalid), None);
        }
    }
}
