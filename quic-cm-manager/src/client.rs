use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
};

use mio::{
    {Poll, Token},
    event::Event,
};

use crate::{
    connection::{Connection, State},
    mio_tokens::TokenManager,
};

pub struct Client {
    socket: UnixStream,
    token: Token,
    connection: Connection,
    established: bool,
    stream_id: u64,
}


impl Client {
    pub fn new(
        address: &str,
        socket: UnixStream,
        controltoken: Token,
        tokenmanager: &mut TokenManager,
        poll: &mut Poll
    ) -> Client {
        Client {
            socket,
            token: controltoken,
            connection: Connection::new(address, tokenmanager, poll),
            established: false,
            stream_id: 4,
        }
    }


    /// Process I/O events from client. Returns false if client has disconnected,
    /// otherwise true.
    pub fn process_events(&mut self, event: &Event) -> Result<bool, String> {
        if event.token() == self.token {
            //debug!("Data to be sent from Fifo");
            let n = match self.process_control_msg() {
                Ok(n) => n,
                Err(e) => return Err(e),
            };
            if n == 0 {
                return Ok(false);
            }
            let ok = *b"OK"; // TODO: send proper response
            self.socket.write(&ok).unwrap(); // TODO: fix error handling
        }

        if event.token() == self.connection.get_token() {
            // TODO: error handling
            self.connection.process_datagram();
            // TODO: specify which streams we are interested in
            if self.established {
                self.deliver_data();
            }
        }
        if let State::Established = self.connection.get_state() {
            if !self.established {
                // Inform client that connection has just been established.
                self.established = true;
                let ok = *b"OK";
                self.socket.write(&ok).unwrap();
            }
        }
        self.connection.send_data();
        Ok(true)
    }


    pub fn cleanup(&self, tokenmanager: &mut TokenManager) {
        tokenmanager.free_token(self.token);
    }


    fn deliver_data(&mut self) {
        let data = self.connection.peek_data(&self.stream_id);
        if data.is_none() {
            return;
        }
        let _n = self.socket.write(&data.unwrap()).unwrap();
        // TODO: error handling
        // TODO: remove processed data from connection
    }


    /// Process control message from Unix domain socket.
    /// Returns number of bytes sent forward, or 0 if the Unix socket is closed
    /// (most likely because the client application has terminated).
    fn process_control_msg(&mut self) -> Result<usize, String> {
        let mut buf = [0; 65535];
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

                let n = match self.socket.read(&mut buf) {
                    Ok(n) => n,
                    Err(e) => {
                        error!("Read from fifo failed: {}", e);
                        return Err(format!("Read from fifo failed: {}", e));
                    },
                };
                debug!("Read {} bytes from control socket", n);
                self.connection.send(self.stream_id, &buf[..n])
            },
            _ => Err(format!("Unknown command: {}", cmdstr)),
        }
    }
}
