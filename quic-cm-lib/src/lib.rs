#[macro_use]
extern crate log;

use tokio::net::UnixStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::common::{QCM_CONTROL_SOCKET, write_data_header};


pub struct QuicClient {
    socket: UnixStream,
}

impl QuicClient {

    /// Initiate QUIC connection to given address
    pub async fn connect(address: &str, app_proto: &str) -> Result<QuicClient, String> {
        let mut socket = match UnixStream::connect(QCM_CONTROL_SOCKET).await {
            Ok(s) => s,
            Err(e) => return Err(format!("Could not open unix socket: {}", e)),
        };

        let v = format!("CONN {} {} ", address, app_proto).as_bytes().to_vec();
        let n = match socket.write(&v).await {
            Ok(n) => n,
            Err(e) => return Err(format!("Control message sending failed: {}", e)),
        };
        debug!("fifo connect, wrote CONN message with {} bytes", n);

        let mut buf = [0; 65535];
        match socket.read(&mut buf).await {
            Ok(n) => {
                if n == 0 {
                    return Err(format!("Control socket closed prematurely"));
                }
                let bufstr = std::str::from_utf8(&buf[..n]).unwrap();
                if bufstr.eq("OK") {
                    Ok(QuicClient{ socket })
                } else {
                    return Err(format!("Received connection error: {}", bufstr));
                }
            },
            Err(e) => {
                return Err(format!("Reading control response failed: {}", e))
            }
        }
    }


    /// Write bytes to QUIC connection.
    pub async fn write(&mut self, buf: &[u8]) -> Result<usize, String> {
        // write "DATA" type heder and u32 length information
        let len: u32 = match buf.len().try_into() {
            Ok(v) => v,
            Err(e) => return Err(format!("length conversion failed: {:?}", e)),
        };
        let n = write_data_header(&mut self.socket, len).await;
        if n.is_err() {
            return Err(format!("Could not write header to Unix socket: {}", n.err().unwrap()));
        }
        debug!("Wrote header, {} bytes", n.unwrap());
        let n = match self.socket.write(buf).await {
            Ok(n) => n,
            Err(e) => return Err(format!("Could not write to Unix socket: {}", e)),
        };
        debug!("Wrote to Unix socket {} bytes", n);

        let mut ok: [u8; 8] = [0; 8];
        let n = self.socket.read(&mut ok).await.unwrap(); // TODO: error handling
        debug!("Write response: {} bytes", n);
        Ok(n)
    }


    pub async fn read(&mut self, buf: &mut [u8]) -> Result<usize, String> {
        let mut header: [u8; 8] = [0; 8];
        let n = self.socket.read(&mut header).await;
        if n.is_err() {
            return Err(format!("Could not read header from Unix socket: {}", n.err().unwrap()));
        }
        debug!("Read header, {} bytes: {}", n.unwrap(), String::from_utf8_lossy(&header));
        let n = match self.socket.read(buf).await {
            Ok(n) => n,
            Err(e) => return Err(format!("Could not read from Unix socket: {}", e)),
        };
        debug!("Read from Unix socket {} bytes", n);
        Ok(n)
    }
}

pub mod common;
