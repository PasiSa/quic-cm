#[macro_use]
extern crate log;

use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
};

use crate::common::{QCM_CONTROL_SOCKET, write_data_header};


pub struct QuicClient {
    socket: UnixStream,
}

impl QuicClient {

    /// Initiate QUIC connection to given address
    pub fn connect(address: &str) -> Result<QuicClient, String> {
        let mut socket = match UnixStream::connect(QCM_CONTROL_SOCKET) {
            Ok(s) => s,
            Err(e) => return Err(format!("Could not open unix socket: {}", e)),
        };

        let v = format!("CONN {} {}", address, 0).as_bytes().to_vec();
        let n = match socket.write(&v) {
            Ok(n) => n,
            Err(e) => return Err(format!("Control message sending failed: {}", e)),
        };
        debug!("fifo connect, wrote CONN message with {} bytes", n);

        let mut buf = [0; 65535];
        match socket.read(&mut buf) {
            Ok(_) => {
                debug!("Read: {}", std::str::from_utf8(&buf).unwrap());
                Ok(QuicClient{ socket })
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
        let n = write_data_header(&mut self.socket, len);
        if n.is_err() {
            return Err(format!("Could not write header to Fifo: {}", n.err().unwrap()));
        }
        debug!("Wrote header, {} bytes", n.unwrap());
        let n = match self.socket.write(buf) {
            Ok(n) => n,
            Err(e) => return Err(format!("Could not write to FIFO: {}", e)),
        };
        debug!("Wrote to fifo {} bytes", n);

        let mut ok: [u8; 8] = [0; 8];
        let n = self.socket.read(&mut ok).unwrap(); // TODO: error handling
        debug!("Write response: {} bytes", n);
        Ok(n)
    }


    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, String> {
        let mut header: [u8; 8] = [0; 8];
        let n = self.socket.read(&mut header);
        if n.is_err() {
            return Err(format!("Could not read header from Fifo: {}", n.err().unwrap()));
        }
        debug!("Read header, {} bytes: {}", n.unwrap(), String::from_utf8_lossy(&header));
        let n = match self.socket.read(buf) {
            Ok(n) => n,
            Err(e) => return Err(format!("Could not read from FIFO: {}", e)),
        };
        debug!("Read from fifo {} bytes", n);
        Ok(0)
    }
}

pub mod common;
