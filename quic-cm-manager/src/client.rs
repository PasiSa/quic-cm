use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
};

use mio::Token;
use quic_cm_lib::common::write_data_header;

use crate::mio_tokens::TokenManager;

pub struct Client {
    socket: UnixStream,
    token: Token,
    readbuf: [u8; 65535],
    readn: usize,
}


impl Client {
    pub fn new(
        socket: UnixStream,
        token: Token,
    ) -> Client {
        Client {
            socket,
            token,
            readbuf: [0; 65535],
            readn: 0,
        }
    }


    pub fn send_ok(&mut self) {
        let ok = *b"OK";
        self.socket.write(&ok).unwrap();
    }


    pub fn cleanup(&self, tokenmanager: &mut TokenManager) {
        tokenmanager.free_token(self.token);
    }


    pub fn deliver_data(&mut self, data: &Vec<u8>) {
        let len: u32 = data.len().try_into().unwrap();
        write_data_header(&mut self.socket, len).unwrap();
        let _n = self.socket.write(&data).unwrap();
        // TODO: error handling
        // TODO: remove processed data from connection
    }


    pub fn get_token(&self) -> Token {
        self.token
    }


    /// Process control message from Unix domain socket.
    /// Returns number of bytes sent forward, or 0 if the Unix socket is closed
    /// (most likely because the client application has terminated).
   pub fn process_control_msg(&mut self) -> Result<usize, String> {
        let mut cmd: [u8; 4] = [0; 4];
        let n = match self.socket.read(&mut cmd) {
            Ok(n) => n,
            Err(e) => {
                error!("Read from fifo failed: {}", e);
                return Err(format!("Read from fifo failed: {}", e));
            },
        };

        if n == 0 {
            return Ok(0);
        }
        if n < 4 {
            return Err(format!("Could not read command from Fifo: {} bytes", n));
        }

        let cmdstr = match String::from_utf8(cmd.to_vec()) {
            Ok(s) => s,
            Err(e) => return Err(format!("Invalid command: {}", e)),
        };

        match cmdstr.as_str() {
            "DATA" => {
                let mut lbuf: [u8; 4] = [0; 4];
                let n = match self.socket.read(&mut lbuf) {
                    Ok(n) => n,
                    Err(e) => return Err(format!("Not valid bytes in DATA message: {}", e)), 
                };
                if n < 4 {
                    return Err(format!("Could not read command from Fifo: {} bytes", n));
                }

                self.readn = match self.socket.read(&mut self.readbuf) {
                    Ok(n) => n,
                    Err(e) => {
                        error!("Read from fifo failed: {}", e);
                        return Err(format!("Read from fifo failed: {}", e));
                    },
                };
                debug!("Read {} bytes from control socket", self.readn);
                Ok(self.readn)
            },
            _ => Err(format!("Unknown command: {}", cmdstr)),
        }
    }


    pub fn fetch_databuf(&mut self) -> (usize, &[u8]) {
        let n = self.readn;
        self.readn = 0;
        (n, &self.readbuf)
    }
}
