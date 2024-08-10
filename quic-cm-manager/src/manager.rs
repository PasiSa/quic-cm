use std::{
    collections::HashMap,
    fs::remove_file,
    io::{Read, Write},
    os::{
        fd::AsRawFd,
        unix::net::UnixListener,
    },
    time::Duration,
};

use mio::{
    {Interest, Poll},
    unix::SourceFd,
};
use mio_signals::{Signals, SignalSet, Signal};

use quic_cm::common::QCM_CONTROL_SOCKET;

use crate::{
    client::Client,
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
        // Set timer to connection with nearest timeout
        let mut timeout: Option<Duration> = None;
        for connection in connections.values() {
            if connection.timeout().is_some() {
                if timeout.is_none() || Some(connection.timeout()) < Some(timeout) {
                    timeout = connection.timeout();
                }
            }
        }

        poll.poll(&mut events, timeout).unwrap();
        if events.is_empty() {
            debug!("Timeout");
            for connection in connections.values_mut() {
                connection.process_events(None, &mut tokenmanager).unwrap();
            }
        }
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
                connection.process_events(Some(event), &mut tokenmanager).unwrap();
            }
        }
        // Remove all closed connections
        connections.retain(|_, val| !val.is_closed());
    }

    tokenmanager.free_token(controltoken);
    let _ = remove_file(QCM_CONTROL_SOCKET);
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

    // TODO: Properly parse the CONN message in common function
    let fields: Vec<&str> = str.split_whitespace().collect();
    let conn = fields.get(0).unwrap();
    if (*conn).ne("CONN") {
        Client::send_socket_error(
            &mut socket,
            format!("Expected CONN message for incoming connection, got: {}", str).as_str()
        );
        socket.write("ERROR Expected CONN message".as_bytes()).unwrap();
        return;
    }
    if fields.len() < 3 {
        Client::send_socket_error(
            &mut socket, format!("Malformed CONN message: {}", str).as_str());
        return;
    }
    let address = fields.get(1).unwrap();
    let app_proto = fields.get(2).unwrap();

    if !connections.contains_key(&String::from(*address)) {
        let conn = match Connection::new(address, app_proto, tokenmanager, poll) {
            Ok(c) => c,
            Err(e) => {
                Client::send_socket_error(
                    &mut socket, format!("Connection creation failed: {e}").as_str());
                return;
            }
        };
        connections.insert(
            String::from(*address), 
            conn,
        );
    }
    let connection = connections.get_mut(&String::from(*address)).unwrap();

    connection.add_client(socket, app_proto, poll, token);

}
