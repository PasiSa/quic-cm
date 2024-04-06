use mio::{Interest, Poll};
use mio::unix::SourceFd;
use mio_signals::{Signals, SignalSet, Signal};

use quic_cm_lib::{
    common::QCM_CONTROL_FIFO,
    fifo::Fifo
};

use crate::{
    client::Client,
    mio_tokens::TokenManager
};


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
                client.process_events(event).unwrap();
            }
        }
    }

    for c in clients.iter() {
        c.cleanup(&mut tokenmanager);
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
    let client = Client::new(address, pidstr, tokenmanager, poll);
    clients.push(client);
}
