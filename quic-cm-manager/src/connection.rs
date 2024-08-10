use std::{
    collections::HashMap,
    net::ToSocketAddrs, os::unix::net::UnixStream,
    os::fd::AsRawFd,
    time::Duration,
};
use mio::{
    event::Event,
    net::UdpSocket,
    unix::SourceFd,
    Interest,
    Poll,
    Token,
};
use ring::rand::*;
use quiche::Config;

use crate::{
    client::Client,
    mio_tokens::TokenManager,
};

const MAX_DATAGRAM_SIZE: usize = 1350;

pub enum State {
    Connecting,
    Established,
    Closed,
}

/// QUIC connection to a server destination. There are separate QUIC-CM clients for
/// each stream opened with the server.
pub struct Connection {
    socket: UdpSocket,
    app_proto: String,
    token: Token,
    qconn: quiche::Connection,
    state: State,
    received_data: HashMap<u64, Vec<u8>>,
    clients: HashMap<u64, Client>,  // Key is QUIC stream ID
    next_stream_id: u64,
}

impl Connection {

    pub fn new(
        address: &str,
        app_proto: &str,
        tokenmanager: &mut TokenManager,
        poll: &mut Poll,
    ) -> Result<Connection, String> {
        let addr = match Self::resolve_address(address) {
            Ok(addr) => addr,
            Err(e) => { return Err(e); },
        };
        let bind_addr = match addr {
            std::net::SocketAddr::V4(_) => "0.0.0.0:0",
            std::net::SocketAddr::V6(_) => "[::]:0",
        };
        let mut socket = UdpSocket::bind(bind_addr.parse().unwrap()).unwrap();
        let local_addr = socket.local_addr().unwrap();

        let mut scid = [0; quiche::MAX_CONN_ID_LEN];
        SystemRandom::new().fill(&mut scid[..]).unwrap();
        let scid = quiche::ConnectionId::from_ref(&scid);

        let mut config = set_quic_config(app_proto);

        let mut conn =
            quiche::connect(None, &scid, local_addr, addr, &mut config)
                .unwrap();

        debug!(
            "connecting to {:} from {:} with scid {}",
            addr,
            socket.local_addr().unwrap(),
            hex_dump(&scid)
        );

        let mut out = [0; MAX_DATAGRAM_SIZE];
        let (write, send_info) = conn.send(&mut out).expect("initial send failed");

        while let Err(e) = socket.send_to(&out[..write], send_info.to) {
            if e.kind() == std::io::ErrorKind::WouldBlock {
                println!("send() would block");
                continue;
            }

            error!("send() failed: {:?}", e);
        }
        debug!("connecting, written {} bytes", write);

        let token = tokenmanager.allocate_token();
        poll.registry()
            .register(&mut socket, token, Interest::READABLE)
            .unwrap();

        Ok(Connection {
            socket: socket,
            app_proto: app_proto.to_string(),
            token: token,
            qconn: conn,
            state: State::Connecting,
            received_data: HashMap::new(),
            clients: HashMap::new(),
            next_stream_id: 4,
        })
    }


    pub fn process_datagram(&mut self) {
        // TODO: error handling
        let mut buf = [0; 65535];
        loop {
            let (len, from) = match self.socket.recv_from(&mut buf) {
                Ok(v) => v,

                Err(e) => {
                    // There are no more UDP packets to read, so end the read
                    // loop.
                    if e.kind() == std::io::ErrorKind::WouldBlock {
                        break;
                    }

                    panic!("recv() failed: {:?}", e);
                },
            };

            let recv_info = quiche::RecvInfo {
                to: self.socket.local_addr().unwrap(),
                from,
            };

            // Process potentially coalesced packets.
            let read = match self.qconn.recv(&mut buf[..len], recv_info) {
                Ok(v) => v,

                Err(e) => {
                    error!("recv failed: {:?}", e);
                    continue;
                },
            };

            debug!("processed from socket {} bytes", read);
        }
        if self.qconn.is_closed() {
            self.state = State::Closed;
            debug!("connection closed, {:?}", self.qconn.stats());
            return;
        }

        if self.qconn.peer_error().is_some() {
            error!("Error occurred when establishing connection");
            for client in self.clients.values_mut() {
                client.send_error("Error occurred when establishing connection");
            }
        }

        if self.qconn.is_established() {
            if let State::Connecting = self.state {
                self.state = State::Established;
                for client in self.clients.values_mut() {
                    client.send_ok();
                }
            }
            self.handle_established();
        }
    }


    /// Process MIO events. If event is None, timeout has occurred.
    pub fn process_events(&mut self, event: Option<&Event>, tokenmanager: &mut TokenManager) -> Result<(), String> {
        if event.is_some() {
            if event.unwrap().token() == self.token {
                // TODO: error handling
                self.process_datagram();
            }

            let mut leaving: Vec<u64> = Vec::new();
            let mut writing: Vec<u64> = Vec::new();
            for (stream_id, client) in self.clients.iter_mut() {
                if event.unwrap().token() == client.get_token() {
                    match client.process_control_msg() {
                        Ok(n) => {
                            if n == 0 {
                                info!("Client leaving");
                                // TODO: close stream
                                client.cleanup(tokenmanager);
                                leaving.push(*stream_id);
                            } else {
                                writing.push(*stream_id);
                                // TODO: send OK response
                                client.send_ok();
                            }
                        },
                        Err(e) => return Err(format!("process client: {}", e)),
                    }
                }
            }
            for c in writing {
                self.send(c).unwrap();  // TODO: handle errors
            }
            for index in leaving {
                self.clients.remove(&index);
            }
        } else {
            self.qconn.on_timeout();
        }

        if self.qconn.is_closed() {
            debug!("Connection is closed");
            for c in self.clients.values_mut() {
                c.send_error("Connection is closed");
            }
        }

        self.send_data();
        Ok(())
    }


    pub fn add_client(&mut self, socket: UnixStream, app_proto: &str, poll: &mut Poll, token: Token) {
        // check that app_proto matches with earlier made connectiom
        if app_proto.ne(&self.app_proto) {
                let mut mutsock = socket;
                Client::send_socket_error(
                    &mut mutsock,
                    format!("Application proto from new client does not match connection.").as_str()
                );
                return;
        }

        poll.registry()
            .register(&mut SourceFd(&socket.as_raw_fd()),
                token, Interest::READABLE)
            .unwrap();

        let stream_id: u64 = self.next_stream_id;
        debug!("add_client, stream ID: {}", stream_id);
        self.next_stream_id += 4;
        let mut client = Client::new(socket, token);
        if let State::Established = self.state {
            client.send_ok();
        }
        self.clients.insert(
            stream_id,
            client
        );
    }


    pub fn send(&mut self, stream_id: u64) -> Result<usize, String> {
        let client = self.clients.get_mut(&stream_id).unwrap();
        let (n, buf) = client.fetch_databuf();
        let written = match self.qconn.stream_send(stream_id, &buf[..n], false) {
            Ok(n) => n,
            Err(quiche::Error::Done) => 0,
            Err(e) => {
                return Err(format!("{} stream send failed {:?}", self.qconn.trace_id(), e));
            },
        };
        debug!("send wrote {} bytes to stream {}", written, stream_id);
        Ok(written)
    }


    pub fn timeout(&self) -> Option<Duration> {
        self.qconn.timeout()
    }


    pub fn is_closed(&self) -> bool {
        self.qconn.is_closed()
    }


    fn resolve_address(address: &str) -> Result<std::net::SocketAddr, String> {
        let mut addrs = match address.to_socket_addrs() {
            Ok(addrs) => {
                addrs
            },
            Err(e) => {
                return Err(format!("Error resolving address '{}': {}", address, e));
            }
        };
        // TODO: needs to be redesigned. Need to check if first datagram is acknowledged,
        // and rotate to new if not.
        // For now only IPv4 addresses are therefore accepted. Fix later.
        let addr = addrs.find(|&x| x.is_ipv4());
        match addr {
            Some(a) => Ok(a),
            None => Err(format!("Could not find acceptable address for: {}", address))
        }
    }


    fn send_data(&mut self) {
        let mut out = [0; MAX_DATAGRAM_SIZE];

        // Generate outgoing QUIC packets and send them on the UDP socket, until
        // quiche reports that there are no more packets to be sent.
        loop {
            let (write, send_info) = match self.qconn.send(&mut out) {
                Ok(v) => v,
                Err(quiche::Error::Done) => break,
                Err(e) => {
                    error!("send failed: {:?}", e);

                    self.qconn.close(false, 0x1, b"fail").ok();
                    break;
                },
            };
            if let Err(e) = self.socket.send_to(&out[..write], send_info.to) {
                if e.kind() == std::io::ErrorKind::WouldBlock {
                    break;
                }
                panic!("send() failed: {:?}", e);
            }

            debug!("written to socket {} bytes", write);
        }
    }


    fn handle_established(&mut self) {
        let mut buf = [0; 65535];

        // Process all readable streams.
        for stream_id in self.qconn.readable() {
            while let Ok((read, fin)) =
                self.qconn.stream_recv(stream_id, &mut buf)
            {
                let stream_buf = &buf[..read];
                debug!(
                    "{} stream {} has {} bytes (fin? {})",
                    self.qconn.trace_id(),
                    stream_id,
                    stream_buf.len(),
                    fin
                );
                println!("Received: {}", String::from_utf8(stream_buf.to_vec()).unwrap());

                if !self.received_data.contains_key(&stream_id) {
                    self.received_data.insert(stream_id, Vec::new());
                }
                let v = self.received_data.get_mut(&stream_id).unwrap();
                v.append(&mut stream_buf.to_vec());
            }
            match self.clients.get_mut(&stream_id) {
                Some(client) => {
                    let v = self.received_data.get_mut(&stream_id).unwrap();
                    match client.deliver_data(v) {
                        Ok(n) => {
                            v.drain(0..n);
                        },
                        Err(e) => {
                            error!("Got error delivering to client: {}", e);
                        },
                    };
                },
                None => {
                    info!("Could not find client for stream {}", stream_id);
                },
            };
            // TODO: remove processed data from connection
        }
    }
}


fn hex_dump(buf: &[u8]) -> String {
    let vec: Vec<String> = buf.iter().map(|b| format!("{b:02x}")).collect();

    vec.join("")
}


fn set_quic_config(appname: &str) -> Config {
    let mut config = quiche::Config::new(quiche::PROTOCOL_VERSION).unwrap();

    config.verify_peer(false);

    config.set_application_protos(&[
            appname.as_bytes(),
        ]).unwrap();

    config.set_max_idle_timeout(50000);
    config.set_max_recv_udp_payload_size(MAX_DATAGRAM_SIZE);
    config.set_max_send_udp_payload_size(MAX_DATAGRAM_SIZE);
    config.set_initial_max_data(10_000_000);
    config.set_initial_max_stream_data_bidi_local(1_000_000);
    config.set_initial_max_stream_data_bidi_remote(1_000_000);
    config.set_initial_max_stream_data_uni(1_000_000);
    config.set_initial_max_streams_bidi(100);
    config.set_initial_max_streams_uni(100);
    config.set_disable_active_migration(true);
    config.enable_early_data();

    config
}
