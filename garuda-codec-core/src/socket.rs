use std::os::fd::RawFd;
use std::ptr;
use std::{ffi, io};

pub struct RawSocket {
    socket_fd: RawFd,
    sockaddr: libc::sockaddr_ll,
}

impl RawSocket {
    pub fn new(interface_name: &str) -> io::Result<RawSocket> {
        #[allow(unsafe_code)]
        let socket_fd =
            unsafe { libc::socket(libc::AF_PACKET, libc::SOCK_RAW, libc::ETH_P_ALL.to_be()) };
        if socket_fd < 0 {
            return Err(io::Error::last_os_error());
        }

        let if_name = match ffi::CString::new(interface_name) {
            Ok(if_name) => if_name,
            Err(_) => {
                return Err(io::Error::other("failed to convert interface name"));
            }
        };

        #[allow(unsafe_code)]
        let if_index = unsafe { libc::if_nametoindex(if_name.as_ptr()) };
        if if_index == 0 {
            return Err(io::Error::last_os_error());
        }

        #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
        let sockaddr = libc::sockaddr_ll {
            sll_family: libc::AF_PACKET as libc::c_ushort,
            sll_protocol: (libc::ETH_P_ALL as libc::c_ushort).to_be(),
            sll_ifindex: if_index as libc::c_int,
            sll_hatype: 0,
            sll_pkttype: 0,
            sll_halen: 0,
            sll_addr: [0; 8],
        };

        Ok(RawSocket {
            socket_fd,
            sockaddr,
        })
    }

    pub fn bind(self) -> io::Result<RawSocketReader> {
        self.set_receive_timeout(1, 0)?;

        #[allow(clippy::cast_possible_truncation, unsafe_code)]
        let result = unsafe {
            libc::bind(
                self.socket_fd,
                (&raw const self.sockaddr).cast::<libc::sockaddr>(),
                size_of::<libc::sockaddr_ll>() as libc::socklen_t,
            )
        };
        if result < 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(RawSocketReader { socket: self })
    }

    fn set_receive_timeout(&self, secs: i64, usecs: i64) -> io::Result<()> {
        let tv = libc::timeval {
            tv_sec: secs,
            tv_usec: usecs,
        };

        #[allow(clippy::cast_possible_truncation, unsafe_code)]
        let result = unsafe {
            libc::setsockopt(
                self.socket_fd,
                libc::SOL_SOCKET,
                libc::SO_RCVTIMEO,
                (&raw const tv).cast(),
                size_of::<libc::timeval>() as u32,
            )
        };
        if result < 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(())
    }
}

impl io::Write for RawSocket {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        #[allow(clippy::cast_possible_truncation, unsafe_code)]
        let bytes_sent = unsafe {
            libc::sendto(
                self.socket_fd,
                buf.as_ptr().cast::<libc::c_void>(),
                buf.len(),
                0,
                (&raw const self.sockaddr).cast::<libc::sockaddr>(),
                size_of::<libc::sockaddr_ll>() as libc::socklen_t,
            )
        };
        if bytes_sent < 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(bytes_sent.cast_unsigned())
    }

    fn flush(&mut self) -> io::Result<()> {
        // Raw sockets are unbuffered at the OS level:
        // writing to the socket sends the data immediately.
        Ok(())
    }
}

impl Drop for RawSocket {
    fn drop(&mut self) {
        #[allow(unsafe_code)]
        let result = unsafe { libc::close(self.socket_fd) };
        if result < 0 {
            let err = io::Error::last_os_error();
            eprintln!(
                "failed to close socket file descriptor {}: {}",
                self.socket_fd, err
            );
        }
    }
}

impl Clone for RawSocket {
    fn clone(&self) -> Self {
        #[allow(unsafe_code)]
        let new_fd = unsafe { libc::dup(self.socket_fd) };

        Self {
            socket_fd: new_fd,
            sockaddr: self.sockaddr,
        }
    }
}

pub struct RawSocketReader {
    socket: RawSocket,
}

impl io::Read for RawSocketReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        #[allow(unsafe_code)]
        let bytes_received = unsafe {
            libc::recvfrom(
                self.socket.socket_fd,
                buf.as_mut_ptr().cast::<libc::c_void>(),
                buf.len(),
                0,
                ptr::null_mut(),
                ptr::null_mut(),
            )
        };
        if bytes_received < 0 {
            return Err(io::Error::last_os_error());
        }

        Ok(bytes_received.cast_unsigned())
    }
}

impl Clone for RawSocketReader {
    fn clone(&self) -> Self {
        Self {
            socket: self.socket.clone(),
        }
    }
}
