use std::{
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
    client::Client,
    mio_tokens::TokenManager
};


pub fn start_manager() {
    let mut tokenmanager: TokenManager = TokenManager::new();
    let mut clients: Vec<Client> = Vec::new();
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
                accept_incoming(&controlsocket, &mut clients,
                                    &mut tokenmanager, &mut poll);
            }
            let mut leaving: Vec<usize> = Vec::new();
            for (index, client) in clients.iter_mut().enumerate() {
                if !client.process_events(event).unwrap() {
                    info!("Client leaving");
                    client.cleanup(&mut tokenmanager);
                    // TODO: close connection
                    leaving.push(index);
                }
            }
            for index in leaving {
                clients.remove(index);
            }
        }
    }

    for c in clients.iter() {
        c.cleanup(&mut tokenmanager);
    }
    tokenmanager.free_token(controltoken);
    remove_file(QCM_CONTROL_SOCKET).unwrap();
}


fn accept_incoming(listener: &UnixListener, clients: &mut Vec<Client>,
    tokenmanager: &mut TokenManager, poll: &mut Poll)
{
    let mut buf = [0; 2048];
    let (mut socket,_) = listener.accept().unwrap();
    let token = tokenmanager.allocate_token();

    socket.read(&mut buf).unwrap();
    let str = std::str::from_utf8(&buf).unwrap();
    debug!("accept -- read: '{}'", str);

    // TODO: Properly parse the CONN message in common function
    let fields: Vec<&str> = str.split_whitespace().collect();
    let address = fields.get(1).unwrap();

    poll.registry()
        .register(&mut SourceFd(&socket.as_raw_fd()),
            token, Interest::READABLE)
        .unwrap();

    clients.push( Client::new(address, socket, token, tokenmanager, poll ));
}
