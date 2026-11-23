//! The debug link's wire on the PC: a local network connection (TCP)
//! instead of the USB cable.
//!
//! Owner decision D6 (taken: follow the recommendation): the simulator
//! should answer the same debug-link commands as the board, over TCP, so
//! one PC tool drives both. This file is only the wire. Answering the
//! commands is the console's job, the same code as on the board.
//!
//! With no `--link`, the cable is "unplugged": nothing arrives, and what
//! the console sends is dropped (as a real UART does with no listener).
//! The last bytes sent are kept for tests.

use std::collections::VecDeque;
use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};

use board_api::Link;

/// Bytes kept of what was sent with nobody listening (for tests).
const KEEP_SENT: usize = 4096;

/// The link: a TCP port on this PC, or unplugged.
#[derive(Default)]
pub struct SimLink {
    listener: Option<TcpListener>,
    client: Option<TcpStream>,
    /// Bytes a test put in, read before anything from the network.
    pub inbox: VecDeque<u8>,
    /// The last bytes sent while no program was connected.
    pub sent: VecDeque<u8>,
}

impl SimLink {
    /// Listens on a local address, for example `127.0.0.1:7000`. One
    /// program at a time can connect. Nothing ever waits.
    pub fn listen(addr: &str) -> std::io::Result<SimLink> {
        let listener = TcpListener::bind(addr)?;
        listener.set_nonblocking(true)?;
        Ok(SimLink {
            listener: Some(listener),
            ..SimLink::default()
        })
    }

    /// The address it listens on (handy with port 0: "any free port").
    pub fn local_addr(&self) -> Option<std::net::SocketAddr> {
        self.listener.as_ref().and_then(|l| l.local_addr().ok())
    }

    /// The connected program, if any (accepts a new one when free).
    fn client(&mut self) -> Option<&mut TcpStream> {
        if self.client.is_none()
            && let Some(l) = &self.listener
            && let Ok((s, _)) = l.accept()
            && s.set_nonblocking(true).is_ok()
        {
            let _ = s.set_nodelay(true);
            self.client = Some(s);
        }
        self.client.as_mut()
    }
}

impl Link for SimLink {
    fn read(&mut self, buf: &mut [u8]) -> usize {
        let mut n = 0;
        while n < buf.len()
            && let Some(b) = self.inbox.pop_front()
        {
            buf[n] = b;
            n += 1;
        }
        if n < buf.len()
            && let Some(c) = self.client()
        {
            match c.read(&mut buf[n..]) {
                Ok(0) => self.client = None, // the program closed it
                Ok(k) => n += k,
                Err(e) if e.kind() == ErrorKind::WouldBlock => {}
                Err(_) => self.client = None,
            }
        }
        n
    }

    fn write(&mut self, data: &[u8]) -> usize {
        if let Some(c) = self.client() {
            return match c.write(data) {
                Ok(k) => k,
                Err(e) if e.kind() == ErrorKind::WouldBlock => 0,
                Err(_) => {
                    self.client = None;
                    data.len()
                }
            };
        }
        // Unplugged: the bytes go nowhere. Keep the last few for tests.
        self.sent.extend(data);
        let extra = self.sent.len().saturating_sub(KEEP_SENT);
        self.sent.drain(..extra);
        data.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unplugged_takes_everything_and_reads_nothing() {
        let mut l = SimLink::default();
        assert_eq!(l.write(b"hello"), 5);
        let mut buf = [0; 8];
        assert_eq!(l.read(&mut buf), 0);
        l.inbox.extend(b"ab");
        assert_eq!(l.read(&mut buf), 2);
        assert_eq!(&buf[..2], b"ab");
        assert_eq!(l.sent.iter().copied().collect::<Vec<_>>(), b"hello");
    }

    #[test]
    fn bytes_go_both_ways_over_tcp() {
        let mut l = SimLink::listen("127.0.0.1:0").unwrap();
        let mut pc = TcpStream::connect(l.local_addr().unwrap()).unwrap();
        pc.write_all(b"ping").unwrap();
        // Nothing waits in the link, so try a few times (the network on
        // the same PC takes a moment).
        let mut buf = [0; 16];
        let mut got = Vec::new();
        for _ in 0..200 {
            let n = l.read(&mut buf);
            got.extend_from_slice(&buf[..n]);
            if got.len() >= 4 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(got, b"ping");
        assert_eq!(l.write(b"pong"), 4);
        let mut back = [0; 4];
        pc.read_exact(&mut back).unwrap();
        assert_eq!(&back, b"pong");
        assert!(l.sent.is_empty());
    }
}
