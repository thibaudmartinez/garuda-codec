#[cfg(target_os = "linux")]
use crate::{RawSocket, RawSocketReader};
use std::io;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::net::{UnixListener, UnixStream};

const STDIN: &str = "-";
const STDOUT: &str = "-";
const TCP_URI_PREFIX: &str = "tcp://";
const UNIX_SOCKET_URI_PREFIX: &str = "unix://";

#[cfg(target_os = "linux")]
const AF_PACKET_SOCKET_URI_PREFIX: &str = "af_packet://";

pub enum OutputStream {
    Stdout,
    Tcp(TcpStream),
    Unix(UnixStream),
    #[cfg(target_os = "linux")]
    RawSocket(RawSocket),
}

impl OutputStream {
    pub fn from_uri(uri: &str) -> io::Result<Self> {
        if uri == STDOUT {
            return Ok(OutputStream::Stdout);
        }

        if let Some(addr) = uri.strip_prefix(TCP_URI_PREFIX) {
            let stream = TcpStream::connect(addr)?;
            return Ok(OutputStream::Tcp(stream));
        }

        if let Some(addr) = uri.strip_prefix(UNIX_SOCKET_URI_PREFIX) {
            let stream = UnixStream::connect(addr)?;
            return Ok(OutputStream::Unix(stream));
        }

        #[cfg(target_os = "linux")]
        if let Some(interface_name) = uri.strip_prefix(AF_PACKET_SOCKET_URI_PREFIX) {
            let socket = RawSocket::new(interface_name)?;
            return Ok(OutputStream::RawSocket(socket));
        }

        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Unsupported output URI: {uri}"),
        ))
    }

    pub fn try_clone(&self) -> io::Result<Self> {
        match self {
            Self::Stdout => Ok(Self::Stdout),
            Self::Tcp(stream) => Ok(Self::Tcp(stream.try_clone()?)),
            Self::Unix(stream) => Ok(Self::Unix(stream.try_clone()?)),
            #[cfg(target_os = "linux")]
            Self::RawSocket(raw_socket) => Ok(Self::RawSocket(raw_socket.clone())),
        }
    }
}

impl Write for OutputStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Self::Stdout => io::stdout().write(buf),
            Self::Tcp(stream) => stream.write(buf),
            Self::Unix(stream) => stream.write(buf),
            #[cfg(target_os = "linux")]
            Self::RawSocket(raw_socket) => raw_socket.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Stdout => io::stdout().flush(),
            Self::Tcp(stream) => stream.flush(),
            Self::Unix(stream) => stream.flush(),
            #[cfg(target_os = "linux")]
            Self::RawSocket(raw_socket) => raw_socket.flush(),
        }
    }
}

pub enum InputStream {
    Stdin,
    Tcp(TcpStream),
    Unix(UnixStream),
    #[cfg(target_os = "linux")]
    RawSocket(RawSocketReader),
}

impl Read for InputStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Stdin => io::stdin().read(buf),
            Self::Tcp(stream) => stream.read(buf),
            Self::Unix(stream) => stream.read(buf),
            #[cfg(target_os = "linux")]
            Self::RawSocket(raw_socket) => raw_socket.read(buf),
        }
    }
}

pub enum Listener {
    Stdin {
        is_closed: bool,
    },
    Tcp(TcpListener),
    Unix(UnixListener),

    #[cfg(target_os = "linux")]
    RawSocket {
        interface_name: String,
    },
}

impl Listener {
    pub fn from_uri(uri: &str) -> io::Result<Self> {
        if uri == STDIN {
            return Ok(Self::Stdin { is_closed: false });
        }

        if let Some(addr) = uri.strip_prefix(TCP_URI_PREFIX) {
            return Ok(Self::Tcp(TcpListener::bind(addr)?));
        }

        if let Some(path) = uri.strip_prefix(UNIX_SOCKET_URI_PREFIX) {
            match std::fs::remove_file(path) {
                Err(ref err) if err.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
                _ => {}
            }

            return Ok(Self::Unix(UnixListener::bind(path)?));
        }

        #[cfg(target_os = "linux")]
        if let Some(interface_name) = uri.strip_prefix(AF_PACKET_SOCKET_URI_PREFIX) {
            return Ok(Self::RawSocket {
                interface_name: interface_name.into(),
            });
        }

        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Unsupported input URI: {uri}"),
        ))
    }

    pub fn incoming(self) -> Incoming {
        Incoming { listener: self }
    }
}

pub struct Incoming {
    listener: Listener,
}

impl Iterator for Incoming {
    type Item = io::Result<InputStream>;

    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.listener {
            Listener::Stdin { is_closed } => {
                if *is_closed {
                    None
                } else {
                    // Return stdin only once, and then consider it closed.
                    *is_closed = true;
                    Some(Ok(InputStream::Stdin))
                }
            }

            Listener::Tcp(listener) => Some(listener.accept().map(|(s, _)| InputStream::Tcp(s))),

            Listener::Unix(listener) => Some(listener.accept().map(|(s, _)| InputStream::Unix(s))),

            #[cfg(target_os = "linux")]
            Listener::RawSocket { interface_name } => match RawSocket::new(interface_name) {
                Ok(socket) => Some(socket.bind().map(InputStream::RawSocket)),
                Err(e) => Some(Err(e)),
            },
        }
    }
}
