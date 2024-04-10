use std::{
    collections::HashMap,
    fs::remove_file,
    io::Read,
    os::{
        fd::AsRawFd,
        unix::net::UnixListener,
    },
};

use mio::{
    {Interest, Poll},
    unix::SourceFd,
};
use mio_signals::{Signals, SignalSet, Signal};

use quic_cm_lib::common::QCM_CONTROL_SOCKET;

use crate::{
    connection::Connection,
    mio_tokens::TokenManager,
};


pub fn start_manager() {
    let mut tokenmanager: TokenManager = TokenManager::new();
    let mut connections: HashMap<String, Connection> = HashMap::new();
    let mut events = mio::Events::with_capacity(1024);
    let mut poll = mio::Poll::new().unwrap();
    let sigset: SignalSet = Signal::Interrupt | Signal::Terminate;
    let mut signals = Signals::new(sigset).unwrap();
    let signal_token = tokenmanager.allocate_token();

    let controlsocket = UnixListener::bind(QCM_CONTROL_SOCKET)
        .unwrap();
    let controltoken = tokenmanager.allocate_token();

    poll.registry()
        .register(&mut SourceFd(&controlsocket.as_raw_fd()),
                controltoken, Interest::READABLE)
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
            if event.token() == controltoken {
                accept_incoming(&controlsocket,
                                    &mut tokenmanager, &mut poll, &mut connections);
            }

            for connection in connections.values_mut() {
                // TODO: handle errors
                connection.process_events(event, &mut tokenmanager).unwrap();
            }
        }
    }

    // TODO: iterate through connections and cleanup clients

    tokenmanager.free_token(controltoken);
    remove_file(QCM_CONTROL_SOCKET).unwrap();
}


fn accept_incoming<'a>(
    listener: &UnixListener,
    tokenmanager: &mut TokenManager,
    poll: &mut Poll,
    connections: &'a mut HashMap<String, Connection>,
) {
    let mut buf = [0; 2048];
    let (mut socket,_) = listener.accept().unwrap();
    let token = tokenmanager.allocate_token();

    socket.read(&mut buf).unwrap();
    let str = std::str::from_utf8(&buf).unwrap();
    debug!("accept -- read: '{}'", str);

    // TODO: Properly parse the CONN message in common function
    let fields: Vec<&str> = str.split_whitespace().collect();
    let address = fields.get(1).unwrap();

    if !connections.contains_key(&String::from(*address)) {
        connections.insert(
            String::from(*address), 
            Connection::new(address, tokenmanager, poll)
        );
    }
    let connection = connections.get_mut(&String::from(*address)).unwrap();

    connection.add_client(socket, poll, token);

}
