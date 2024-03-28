use std::collections::HashSet;
use std::str::FromStr;

use mio::unix::SourceFd;
use mio::{Interest, Token};

use crate::common::{MIO_QCM_CONTROL, QCM_CLIENT_FIFO, QCM_CONTROL_FIFO};
use crate::fifo::Fifo;

struct Client {
    fifo: Fifo,
    pid: u32,
}

// TODO: implement cleanup of FIFOs

pub fn start_manager() {
    let mut control_fifo = Fifo::new(QCM_CONTROL_FIFO, MIO_QCM_CONTROL)
        .unwrap();
    let mut clients: Vec<Client> = Vec::new();
    let mut events = mio::Events::with_capacity(1024);
    let mut poll = mio::Poll::new().unwrap();
    let mut tokenmanager: TokenManager = TokenManager::new();
    
    let fd = control_fifo.get_fd();
    poll.registry()
        .register(&mut SourceFd(&fd), MIO_QCM_CONTROL, Interest::READABLE)
        .unwrap();

    loop {
        poll.poll(&mut events, None).unwrap();
        for event in &events {
            if event.token() == control_fifo.get_token() {
                process_control_fifo(&mut control_fifo, &mut clients, &mut tokenmanager);
            }
        }
    }
}


/// Control fifo contains messages from clients. Nothing is sent to other direction.
/// Messages are of form "CONN address_string process ID".
/// As a result, a client-specific FIFO is created, and QUIC connection is created
/// Outcome is reported back in client-specific FIFO.
fn process_control_fifo(fifo: &mut Fifo, clients: &mut Vec<Client>,
                        tokenmanager: &mut TokenManager) {
    let mut buf = [0; 65535];
    fifo.read(&mut buf).unwrap();
    debug!("Read: {}", std::str::from_utf8(&buf).unwrap());
    let str = std::str::from_utf8(&buf).unwrap();
    let fields: Vec<&str> = str.split_whitespace().collect();
    if let Some(second_field) = fields.get(1) {
        debug!("The second field is: {}", second_field);
    } else {
        debug!("The second field does not exist.");
    }
    // TODO: error handling
    
    // create client FIFO
    let pidstr = fields.get(2).unwrap();
    let fifoname = String::from(QCM_CLIENT_FIFO) + "-" + pidstr;
    let client = Client{
        fifo: Fifo::new(fifoname.as_str(), tokenmanager.allocate_token()).unwrap(),
        pid: pidstr.parse::<u32>().unwrap(),
     };
     client.fifo.write(String::from_str("OK").unwrap()).unwrap();
     clients.push(client);

    // TODO: Start QUIC connection

}


struct TokenManager {
    used_tokens: HashSet<Token>,
    free_tokens: Vec<Token>,
    next: usize,
}

impl TokenManager {
    fn new() -> TokenManager {
        TokenManager {
            used_tokens: HashSet::new(),
            free_tokens: Vec::new(),
            next: 0,
        }
    }

    fn allocate_token(&mut self) -> Token {
        if let Some(token) = self.free_tokens.pop() {
            self.used_tokens.insert(token);
            token
        } else {
            let token = Token(self.next);
            self.next += 1;
            self.used_tokens.insert(token);
            token
        }
    }

    fn free_token(&mut self, token: Token) {
        if self.used_tokens.remove(&token) {
            self.free_tokens.push(token);
        }
    }
}
