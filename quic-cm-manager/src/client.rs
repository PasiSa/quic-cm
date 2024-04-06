use mio::{Interest, Poll};
use mio::event::Event;
use mio::unix::SourceFd;

use quic_cm_lib::{
    common::QCM_CLIENT_FIFO,
    fifo::Fifo,
};

use crate::{
    connection::{Connection, State},
    mio_tokens::TokenManager,
};

pub struct Client {
    sendfifo: Fifo,
    recvfifo: Fifo,
    connection: Connection,
    established: bool,
    stream_id: u64,
}


impl Client {
    pub fn new(
        address: &str,
        pidstr: &str,
        tokenmanager: &mut TokenManager,
        poll: &mut Poll
    ) -> Client {
        let mut sendfifoname = String::from(QCM_CLIENT_FIFO) + "-" + pidstr + "-send";
        let mut recvfifoname = String::from(QCM_CLIENT_FIFO) + "-" + pidstr + "-recv";

        // The NUL terminator in string from FIFO seems to cause problems, remove it
        sendfifoname = sendfifoname.replace("\0", "");
        recvfifoname = recvfifoname.replace("\0", "");
        let fifo = Fifo::connect(sendfifoname.as_str(), tokenmanager.allocate_token()).unwrap();
        poll.registry()
            .register(&mut SourceFd(&fifo.get_fd()), fifo.get_token(), Interest::READABLE)
            .unwrap();

        Client{
            sendfifo: fifo,
            recvfifo: Fifo::connect(&recvfifoname.as_str(), tokenmanager.allocate_token()).unwrap(),
            connection: Connection::new(address, tokenmanager, poll),
            established: false,
            stream_id: 4,
        }
    }

    pub fn process_events(&mut self, event: &Event) -> Result<(), String> {
        if event.token() == self.sendfifo.get_token() {
            //debug!("Data to be sent from Fifo");
            match self.process_from_fifo() {
                Ok(_) => (),
                Err(e) => return Err(e),
            };
            let ok = *b"OK";
            self.recvfifo.write(&ok).unwrap(); // TODO: fix error handling            
        }
        if event.token() == self.connection.get_token() {
            // TODO: error handling
            self.connection.process_datagram();
            // TODO: specify which streams we are interested in
            self.connection.deliver_to_fifo(&mut self.recvfifo).unwrap();
        }
        if let State::Established = self.connection.get_state() {
            if !self.established {
                // Inform client that connection has just been established.
                self.established = true;
                let ok = *b"OK";
                self.recvfifo.write(&ok).unwrap();
            }
        }
        self.connection.send_data();
        Ok(())
    }


    pub fn cleanup(&self, tokenmanager: &mut TokenManager) {
        tokenmanager.free_token(self.sendfifo.get_token());
        self.sendfifo.cleanup();
    }


    fn process_from_fifo(&mut self) -> Result<usize, String> {
        let mut buf = [0; 65535];
        let mut cmd: [u8; 4] = [0; 4];
        let n = match self.sendfifo.read(&mut cmd) {
            Ok(n) => n,
            Err(e) => {
                error!("Read from fifo failed: {}", e);
                return Err(format!("Read from fifo failed: {}", e));
            },
        };
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
                let n = match self.sendfifo.read(&mut lbuf) {
                    Ok(n) => n,
                    Err(e) => return Err(format!("Not valid bytes in DATA message: {}", e)), 
                };
                if n < 4 {
                    return Err(format!("Could not read command from Fifo: {} bytes", n));
                }

                let _n = match self.sendfifo.read(&mut buf) {
                    Ok(n) => n,
                    Err(e) => {
                        error!("Read from fifo failed: {}", e);
                        return Err(format!("Read from fifo failed: {}", e));
                    },
                };
                self.connection.send(self.stream_id, &buf)
            },
            _ => Err(format!("Unknown command: {}", cmdstr)),
        }
    }
}
