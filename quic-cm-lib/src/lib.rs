#[macro_use]
extern crate log;

use mio::Token;

use crate::common::{QCM_CONTROL_FIFO, QCM_CLIENT_FIFO};
use crate::fifo::Fifo;

pub struct QuicClient {
    fifo: Fifo,
}

impl QuicClient {

    /// Initiate QUIC connection to given address
    pub fn connect(address: &str) -> Result<QuicClient, String> {
        let pid = std::process::id();
        let fifoname = format!("{}-{}", QCM_CLIENT_FIFO, pid);
        let mut client_fifo = Fifo::new(fifoname.as_str(), Token(2));
        if client_fifo.is_err() {
            return Err(format!("Creating client fifo failed: {}", client_fifo.err().unwrap()));
        }

        let control_fifo = Fifo::connect(QCM_CONTROL_FIFO, Token(1));
        if control_fifo.is_err() {
            return Err(format!("Connecting control fifo failed: {}", control_fifo.err().unwrap()));
        }

        let v = format!("CONN {} {}", address, pid).as_bytes().to_vec();
        let n = match control_fifo.unwrap().write(&v) {
            Ok(n) => n,
            Err(e) => return Err(format!("Control message sending failed: {}", e)),
        };
        debug!("fifo connect, wrote CONN message with {} bytes", n);

        let mut buf = [0; 65535];
        match client_fifo.as_mut().unwrap().read(&mut buf) {
            Ok(_) => {
                debug!("Read: {}", std::str::from_utf8(&buf).unwrap());
                Ok(QuicClient{ fifo: client_fifo.unwrap() })
            },
            Err(e) => {
                return Err(format!("Reading control response failed: {}", e))
            }
        }
    }

    /// Write bytes to QUIC connection.
    pub fn write(&mut self, buf: &[u8]) -> Result<usize, String> {
        let send = *b"SEND";
        let n = self.fifo.write(&send[..4]);
        if n.is_err() {
            return Err(format!("Could not write SEND command to Fifo: {}", n.err().unwrap()));
        }
        debug!("Wrote SEND, {} bytes", n.unwrap());
        let n = match self.fifo.write(buf) {
            Ok(n) => n,
            Err(e) => return Err(format!("Could not write to FIFO: {}", e)),
        };
        debug!("Wrote to fifo {} bytes", n);
        Ok(n)
    }
}

pub mod common;
pub mod fifo;
