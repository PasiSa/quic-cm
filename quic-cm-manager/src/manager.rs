use mio::{Interest, Poll};
use mio::unix::SourceFd;
use mio_signals::{Signals, SignalSet, Signal};

use quic_cm_lib::common::{QCM_CLIENT_FIFO, QCM_CONTROL_FIFO};
use quic_cm_lib::fifo::Fifo;

use crate::connection::{Connection, State};
use crate::mio_tokens::TokenManager;

struct Client {
    fifo: Fifo,
    connection: Connection,
    established: bool,
}


pub fn start_manager() {
    let mut tokenmanager: TokenManager = TokenManager::new();
    let mut control_fifo = Fifo::new(QCM_CONTROL_FIFO, tokenmanager.allocate_token())
        .unwrap();
    let mut clients: Vec<Client> = Vec::new();
    let mut events = mio::Events::with_capacity(1024);
    let mut poll = mio::Poll::new().unwrap();
    let sigset: SignalSet = Signal::Interrupt | Signal::Terminate;
    let mut signals = Signals::new(sigset).unwrap();
    let signal_token = tokenmanager.allocate_token();

    poll.registry()
        .register(&mut SourceFd(&control_fifo.get_fd()),
                control_fifo.get_token(), Interest::READABLE)
        .unwrap();

    poll.registry()
        .register(&mut signals, signal_token, Interest::READABLE)
        .unwrap();

    let mut terminate = false;
    while !terminate {
        poll.poll(&mut events, None).unwrap();
        for event in &events {
            if event.token() == signal_token {
                debug!("Signal received");
                terminate = true;
            }
            if event.token() == control_fifo.get_token() {
                process_control_fifo(&mut control_fifo, &mut clients,
                                    &mut tokenmanager, &mut poll);
            }
            for client in &mut clients {
                if event.token() == client.fifo.get_token() {
                    debug!("Data to be sent from Fifo");
                    client.connection.send_from_fifo(&mut client.fifo).unwrap();
                }
                if event.token() == client.connection.get_token() {
                    // TODO: error handling
                    client.connection.process_datagram();
                }
                if let State::Established = client.connection.get_state() {
                    if !client.established {
                        // Inform client that connection has just been established.
                        client.established = true;
                        let ok = *b"OK";
                        client.fifo.write(&ok).unwrap();
                    }
                }
                client.connection.send_data();
            }
        }
    }

    for c in clients.iter() {
        tokenmanager.free_token(c.fifo.get_token());
        c.fifo.cleanup();
    }
    tokenmanager.free_token(control_fifo.get_token());
    control_fifo.cleanup();
}


/// Control fifo contains messages from clients. Nothing is sent to other direction.
/// Messages are of form "CONN address_string process ID".
/// As a result, a client-specific FIFO is created, and QUIC connection is created
/// Outcome is reported back in client-specific FIFO.
fn process_control_fifo(fifo: &mut Fifo, clients: &mut Vec<Client>,
                        tokenmanager: &mut TokenManager, poll: &mut Poll) {
    let mut buf = [0; 65535];
    fifo.read(&mut buf).unwrap();
    let str = std::str::from_utf8(&buf).unwrap();
    debug!("Read: '{}'", str);
    let fields: Vec<&str> = str.split_whitespace().collect();
    let address = fields.get(1).unwrap();

    // TODO: error handling

    // create client FIFO
    let pidstr = fields.get(2).unwrap();
    let mut fifoname = String::from(QCM_CLIENT_FIFO) + "-" + pidstr;

    // The NUL terminator in string from FIFO seems to cause problems, remove it
    fifoname = fifoname.replace("\0", "");
    let fifo = Fifo::connect(fifoname.as_str(), tokenmanager.allocate_token()).unwrap();
    poll.registry()
        .register(&mut SourceFd(&fifo.get_fd()), fifo.get_token(), Interest::READABLE)
        .unwrap();

    let client = Client{
        fifo: fifo,
        connection: Connection::new(address, tokenmanager, poll),
        established: false,
     };
     clients.push(client);
}
