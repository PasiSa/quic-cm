#[macro_use]
extern crate log;

use mio::Token;

use crate::common::{QCM_CONTROL_FIFO, QCM_CLIENT_FIFO};
use crate::fifo::Fifo;

pub struct QuicClient {
    send_fifo: Fifo,
    recv_fifo: Fifo,
}

impl QuicClient {

    /// Initiate QUIC connection to given address
    pub fn connect(address: &str) -> Result<QuicClient, String> {
        let pid = std::process::id();
        let fifoname = format!("{}-{}-send", QCM_CLIENT_FIFO, pid);
        let send_fifo = Fifo::new(fifoname.as_str(), Token(2));
        if send_fifo.is_err() {
            return Err(format!("Creating send fifo failed: {}", send_fifo.err().unwrap()));
        }

        let fifoname = format!("{}-{}-recv", QCM_CLIENT_FIFO, pid);
        let mut recv_fifo = Fifo::new(fifoname.as_str(), Token(2));
        if recv_fifo.is_err() {
            return Err(format!("Creating recv fifo failed: {}", recv_fifo.err().unwrap()));
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
        match recv_fifo.as_mut().unwrap().read(&mut buf) {
            Ok(_) => {
                debug!("Read: {}", std::str::from_utf8(&buf).unwrap());
                Ok(QuicClient{
                    send_fifo: send_fifo.unwrap(), recv_fifo: recv_fifo.unwrap() })
            },
            Err(e) => {
                return Err(format!("Reading control response failed: {}", e))
            }
        }
    }


    /// Write bytes to QUIC connection.
    pub fn write(&mut self, buf: &[u8]) -> Result<usize, String> {
        // write "DATA" type heder and u32 length information
        let len: u32 = match buf.len().try_into() {
            Ok(v) => v,
            Err(e) => return Err(format!("length conversion failed: {:?}", e)),
        };
        let n = self.send_fifo.write_data_header(len);
        if n.is_err() {
            return Err(format!("Could not write header to Fifo: {}", n.err().unwrap()));
        }
        debug!("Wrote header, {} bytes", n.unwrap());
        let n = match self.send_fifo.write(buf) {
            Ok(n) => n,
            Err(e) => return Err(format!("Could not write to FIFO: {}", e)),
        };
        debug!("Wrote to fifo {} bytes", n);

        let mut ok: [u8; 8] = [0; 8];
        let n = self.recv_fifo.read(&mut ok).unwrap(); // TODO: error handling
        debug!("Write response: {} bytes", n);
        Ok(n)
    }


    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, String> {
        let mut header: [u8; 8] = [0; 8];
        let n = self.recv_fifo.read(&mut header);
        if n.is_err() {
            return Err(format!("Could not read header from Fifo: {}", n.err().unwrap()));
        }
        debug!("Read header, {} bytes: {}", n.unwrap(), String::from_utf8_lossy(&header));
        let n = match self.recv_fifo.read(buf) {
            Ok(n) => n,
            Err(e) => return Err(format!("Could not read from FIFO: {}", e)),
        };
        debug!("Read from fifo {} bytes", n);
        Ok(0)
    }
}

pub mod common;
pub mod fifo;
