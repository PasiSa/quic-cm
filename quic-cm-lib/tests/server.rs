use std::net::SocketAddr;
use std::io::{stdin, Read};
use std::collections::{HashMap, HashSet};
use std::time::Duration;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use mio_signals::{Signals, SignalSet, Signal};
use mio::{Interest, Poll, Token};
use mio::net::UdpSocket;
use quiche::Config;
use quiche::Connection;
use ring::rand::*;

// TODO: For now this is just a horrible copy from quiccat code, to implement some sort
// of simple QUIC server. Should be cleaned up appropriately to be good for general tests

pub const MAX_DATAGRAM_SIZE: usize = 1350;
pub const DEFAULT_SERVER_PORT: u16 = 7878;
pub const MIO_SOCKET: Token = Token(0);
pub const MIO_STDIN: Token = Token(1);
pub const MIO_SIGNAL: Token = Token(2);

pub fn set_quic_config() -> Config {
    let mut config = quiche::Config::new(quiche::PROTOCOL_VERSION).unwrap();

    config.verify_peer(false);

    config.set_application_protos(&[
            b"quiccat",
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

pub fn create_socket(peer_addr: SocketAddr, local_port: u16) -> UdpSocket {
    let bind_addr = match peer_addr {
        std::net::SocketAddr::V4(_) => "0.0.0.0",
        std::net::SocketAddr::V6(_) => "[::]",
    };
    let addr = format!("{}:{}", bind_addr, local_port);
    let socket =
        UdpSocket::bind(addr.parse().unwrap()).unwrap();

    socket
}


pub fn set_mio_sources(socket: &mut UdpSocket, signals: &mut Signals) -> Poll {
    let poll = mio::Poll::new().unwrap();

    poll.registry()
        .register(socket, MIO_SOCKET, Interest::READABLE)
        .unwrap();

    /*poll.registry()
        .register(&mut SourceFd(&stdin().as_raw_fd()),
        MIO_STDIN, Interest::READABLE)
        .unwrap();*/

    poll.registry()
        .register(signals, MIO_SIGNAL, Interest::READABLE)
        .unwrap();

    poll
}

struct PartialResponse {
    body: Vec<u8>,
    written: usize,
}

pub struct SockState {
    pub conn: Connection,
    partial_responses: HashMap<u64, PartialResponse>,
    streams: HashSet<u64>,  // currently active stream IDs
}

impl SockState {
    pub fn new(conn: Connection) -> SockState {
        SockState {
            conn,
            partial_responses: HashMap::new(),
            streams: HashSet::new(),
        }
    }

    /// Handle stdin events and signals from OS.
    /// 
    /// Returns true, if application should terminate, false if it should continue.
    pub fn handle_events(
        &mut self, 
        events: &mio::Events,
        stream_id: u64,
        signals: &mut Signals,
        multistream: bool,
    ) -> bool {
        for event in events {
            if event.token() == MIO_STDIN {
                let mut inbuf = [0; 1024];
                match stdin().read(&mut inbuf) {
                    Ok(n) => {
                        //debug!("stdin Read {} bytes: {:?}", n, &inbuf[..n]);
                        if multistream {
                            let streams: Vec<u64> = self.streams.iter().cloned().collect();
                            for stream in streams {
                                self.write(stream, &inbuf[..n]);
                            }
                        } else {
                            self.write(stream_id, &inbuf[..n]);
                        }
                    }
                    Err(e) => println!("Error reading from stdin: {:?}", e),
                }
            }
            if event.token() == MIO_SIGNAL {
                //debug!("Got signal");
                let s = signals.receive().unwrap();
                if s == Some(Signal::Interrupt) || s == Some(Signal::Terminate) {
                    self.conn.close(false, 0, b"Ok").ok();
                    return true;
                }
            }
        }

        false
    }


    pub fn handle_established(&mut self) {
        let mut buf = [0; 65535];
    
        // Process all readable streams.
        for s in self.conn.readable() {
            while let Ok((read, _fin)) =
                self.conn.stream_recv(s, &mut buf)
            {
                //debug!("{} received {} bytes", self.conn.trace_id(), read);

                self.streams.insert(s);
                let stream_buf = &buf[..read];
                /*debug!(
                    "{} stream {} has {} bytes (fin? {})",
                    self.conn.trace_id(), s, stream_buf.len(), fin
                );*/
                // TODO: handle closing streams

                let str = String::from_utf8(stream_buf.to_vec()).unwrap();
                //debug!("from stream {}: ", s);
                print!("{}", str);
            }
            //let wbytes = [0; 5];
            //self.write(s, &wbytes);
        }
    }


    /// Handles newly writable streams.
    pub fn handle_writable(&mut self, stream_id: u64) {
        //debug!("{} stream {} is writable", self.conn.trace_id(), stream_id);

        if !self.partial_responses.contains_key(&stream_id) {
            return;
        }

        let resp = self.partial_responses.get_mut(&stream_id).unwrap();
        let body = &resp.body[resp.written..];

        let written = match self.conn.stream_send(stream_id, body, false) {
            Ok(v) => v,

            Err(quiche::Error::Done) => 0,

            Err(e) => {
                self.partial_responses.remove(&stream_id);

                println!("{} stream send failed {:?}", self.conn.trace_id(), e);
                return;
            },
        };

        resp.written += written;

        if resp.written == resp.body.len() {
            self.partial_responses.remove(&stream_id);
        }
    }


    fn write(&mut self, stream_id: u64, buf: &[u8]) {
        let written = match self.conn.stream_send(stream_id, buf, false) {
            Ok(v) => v,
    
            Err(quiche::Error::Done) => 0,
    
            Err(_e) => {
                //error!("{} stream send failed {:?}", self.conn.trace_id(), e);
                return;
            },
        };
    
        //debug!("written: {} ; buf.len: {}", written, buf.len());
        if written < buf.len() {
            let response = PartialResponse { body: buf.to_vec(), written };
            self.partial_responses.insert(stream_id, response);
        }
    }
}


struct Client {
    sockstate: SockState,
}

type ClientMap = HashMap<quiche::ConnectionId<'static>, Client>;

pub fn server(terminate_signal: Arc<AtomicBool>) {
    let mut buf = [0; 65535];
    let mut out = [0; MAX_DATAGRAM_SIZE];

    // Create the UDP listening socket, and register it with the event loop.
    let addr: SocketAddr = "0.0.0.0:0".parse().unwrap();
    let mut socket = create_socket(addr, DEFAULT_SERVER_PORT);

    // Create the configuration for the QUIC connections.
    let mut config = set_quic_config();
    let path = env!("CARGO_MANIFEST_DIR");
    config.load_cert_chain_from_pem_file(format!("{}/{}", path, "tests/cert.crt").as_str()).expect("Failed to load certificate");
    config.load_priv_key_from_pem_file((format!("{}/{}", path, "tests/cert.key")).as_str()).expect("Failed to load private key");

    let rng = SystemRandom::new();
    let conn_id_seed =
        ring::hmac::Key::generate(ring::hmac::HMAC_SHA256, &rng).unwrap();

    let mut clients = ClientMap::new();

    // Setup the event loop.
    let mut events = mio::Events::with_capacity(1024);
    let local_addr = socket.local_addr().unwrap();
    let sigset: SignalSet = Signal::Interrupt | Signal::Terminate;
    let mut signals = Signals::new(sigset).unwrap();
    let mut poll = set_mio_sources(&mut socket, &mut signals);

    let mut terminate: bool = false;
    while !terminate {
        if terminate_signal.load(Ordering::SeqCst) {
            println!("Termination signal received. Shutting down server...");
            break;
        }
        // Find the shorter timeout from all the active connections.
        //
        // TODO: use event loop that properly supports timers
        //let timeout = clients.values().filter_map(|c| c.sockstate.conn.timeout()).min();
        let timeout = Duration::new(1, 0);
        poll.poll(&mut events, Some(timeout)).unwrap();

        // Check for signals also when there are no clients (TODO: this whole loop needs refactoring)
        if clients.is_empty() {
            for event in &events {
                if event.token() == MIO_SIGNAL {
                    //debug!("Got signal, no connection");
                    terminate = true;
                }
            }
        }

        // we actually can support only one client at a time,
        // hence next() is ok. (TODO: the event loop needs refactoring)
        if let Some(c) = clients.values_mut().next() {
            terminate = c.sockstate.handle_events(
                &events, 7, &mut signals, false);
            for stream_id in c.sockstate.conn.writable() {
                c.sockstate.handle_writable(stream_id);
            }
        }

        // Read incoming UDP packets from the socket and feed them to quiche,
        // until there are no more packets to read.
        'read: loop {
            // If the event loop reported no events, it means that the timeout
            // has expired, so handle it without attempting to read packets. We
            // will then proceed with the send loop.
            if events.is_empty() {
                //debug!("timed out");
                if terminate_signal.load(Ordering::SeqCst) {
                    println!("Termination signal received. Shutting down server...");
                    break;
                }

                clients.values_mut().for_each(|c| c.sockstate.conn.on_timeout());

                break 'read;
            }

            let (len, from) = match socket.recv_from(&mut buf) {
                Ok(v) => v,

                Err(e) => {
                    // There are no more UDP packets to read, so end the read
                    // loop.
                    if e.kind() == std::io::ErrorKind::WouldBlock {
                        //debug!("recv() would block");
                        break 'read;
                    } else {
                        panic!("recv() failed: {:?}", e);
                    }
                },
            };

            //debug!("got {} bytes", len);

            let pkt_buf = &mut buf[..len];

            // Parse the QUIC packet's header.
            let hdr = match quiche::Header::from_slice(
                pkt_buf,
                quiche::MAX_CONN_ID_LEN,
            ) {
                Ok(v) => v,

                Err(_e) => {
                    //error!("Parsing packet header failed: {:?}", e);
                    continue 'read;
                },
            };

            //trace!("got packet {:?}", hdr);

            let conn_id = ring::hmac::sign(&conn_id_seed, &hdr.dcid);
            let conn_id = &conn_id.as_ref()[..quiche::MAX_CONN_ID_LEN];
            let conn_id = conn_id.to_vec().into();

            // Lookup a connection based on the packet's connection ID. If there
            // is no connection matching, create a new one.
            let client = if !clients.contains_key(&hdr.dcid) &&
                !clients.contains_key(&conn_id)
            {
                if hdr.ty != quiche::Type::Initial {
                    //error!("Packet is not Initial");
                    continue 'read;
                }

                if !quiche::version_is_supported(hdr.version) {
                    //warn!("Doing version negotiation");

                    let len =
                        quiche::negotiate_version(&hdr.scid, &hdr.dcid, &mut out)
                            .unwrap();

                    let out = &out[..len];

                    if let Err(e) = socket.send_to(out, from) {
                        if e.kind() == std::io::ErrorKind::WouldBlock {
                            //debug!("send() would block");
                            break;
                        }

                        panic!("send() failed: {:?}", e);
                    }
                    continue 'read;
                }

                let mut scid = [0; quiche::MAX_CONN_ID_LEN];
                scid.copy_from_slice(&conn_id);

                let scid = quiche::ConnectionId::from_ref(&scid);

                // Token is always present in Initial packets.
                let token = hdr.token.as_ref().unwrap();

                // Do stateless retry if the client didn't send a token.
                if token.is_empty() {
                    //warn!("Doing stateless retry");

                    let new_token = mint_token(&hdr, &from);

                    let len = quiche::retry(
                        &hdr.scid,
                        &hdr.dcid,
                        &scid,
                        &new_token,
                        hdr.version,
                        &mut out,
                    )
                    .unwrap();

                    let out = &out[..len];

                    if let Err(e) = socket.send_to(out, from) {
                        if e.kind() == std::io::ErrorKind::WouldBlock {
                            //debug!("send() would block");
                            break;
                        }

                        panic!("send() failed: {:?}", e);
                    }
                    continue 'read;
                }

                let odcid = validate_token(&from, token);

                // The token was not valid, meaning the retry failed, so
                // drop the packet.
                if odcid.is_none() {
                    //error!("Invalid address validation token");
                    continue 'read;
                }

                if scid.len() != hdr.dcid.len() {
                    //error!("Invalid destination connection ID");
                    continue 'read;
                }

                // Reuse the source connection ID we sent in the Retry packet,
                // instead of changing it again.
                let scid = hdr.dcid.clone();

                //debug!("New connection: dcid={:?} scid={:?}", hdr.dcid, scid);

                let conn = quiche::accept(
                    &scid,
                    odcid.as_ref(),
                    local_addr,
                    from,
                    &mut config,
                )
                .unwrap();

                let client = Client {
                    //conn,
                    sockstate: SockState::new(conn),
                    //partial_responses: HashMap::new(),
                };

                clients.insert(scid.clone(), client);

                clients.get_mut(&scid).unwrap()
            } else {
                match clients.get_mut(&hdr.dcid) {
                    Some(v) => v,

                    None => clients.get_mut(&conn_id).unwrap(),
                }
            };

            let recv_info = quiche::RecvInfo {
                to: socket.local_addr().unwrap(),
                from,
            };

            // Process potentially coalesced packets.
            let _read = match client.sockstate.conn.recv(pkt_buf, recv_info) {
                Ok(v) => v,

                Err(_e) => {
                    //error!("{} recv failed: {:?}", client.sockstate.conn.trace_id(), e);
                    continue 'read;
                },
            };

            //debug!("{} processed {} bytes", client.sockstate.conn.trace_id(), read);

            if client.sockstate.conn.is_in_early_data() || client.sockstate.conn.is_established() {
                // Handle writable streams.
                //debug!("calling handle writable");
                for stream_id in client.sockstate.conn.writable() {
                    client.sockstate.handle_writable(stream_id);
                }
                assert!(true, "established");
                client.sockstate.handle_established();
            }
        }

        // Generate outgoing QUIC packets for all active connections and send
        // them on the UDP socket, until quiche reports that there are no more
        // packets to be sent.
        for client in clients.values_mut() {
            loop {
                let (write, send_info) = match client.sockstate.conn.send(&mut out) {
                    Ok(v) => v,

                    Err(quiche::Error::Done) => {
                        //debug!("{} done writing", client.sockstate.conn.trace_id());
                        break;
                    },

                    Err(_e) => {
                        //error!("{} send failed: {:?}", client.sockstate.conn.trace_id(), e);

                        client.sockstate.conn.close(false, 0x1, b"fail").ok();
                        break;
                    },
                };

                if let Err(e) = socket.send_to(&out[..write], send_info.to) {
                    if e.kind() == std::io::ErrorKind::WouldBlock {
                        //debug!("send() would block");
                        break;
                    }

                    panic!("send() failed: {:?}", e);
                }

                //debug!("{} written {} bytes", client.sockstate.conn.trace_id(), write);
            }
        }

        // Garbage collect closed connections.
        clients.retain(|_, ref mut c| {
            //debug!("Collecting garbage");

            /*if c.sockstate.conn.is_closed() {
                debug!(
                    "{} connection collected {:?}",
                    c.sockstate.conn.trace_id(),
                    c.sockstate.conn.stats()
                );
            }*/

            !c.sockstate.conn.is_closed()
        });
    }
}


/// Generate a stateless retry token.
///
/// The token includes the static string `"quiche"` followed by the IP address
/// of the client and by the original destination connection ID generated by the
/// client.
///
/// Note that this function is only an example and doesn't do any cryptographic
/// authenticate of the token. *It should not be used in production system*.
fn mint_token(hdr: &quiche::Header, src: &SocketAddr) -> Vec<u8> {
    let mut token = Vec::new();

    token.extend_from_slice(b"quiche");

    let addr = match src.ip() {
        std::net::IpAddr::V4(a) => a.octets().to_vec(),
        std::net::IpAddr::V6(a) => a.octets().to_vec(),
    };

    token.extend_from_slice(&addr);
    token.extend_from_slice(&hdr.dcid);

    token
}


/// Validates a stateless retry token.
///
/// This checks that the ticket includes the `"quiche"` static string, and that
/// the client IP address matches the address stored in the ticket.
///
/// Note that this function is only an example and doesn't do any cryptographic
/// authenticate of the token. *It should not be used in production system*.
fn validate_token<'a>(
    src: &SocketAddr, token: &'a [u8],
) -> Option<quiche::ConnectionId<'a>> {
    if token.len() < 6 {
        return None;
    }

    if &token[..6] != b"quiche" {
        return None;
    }

    let token = &token[6..];

    let addr = match src.ip() {
        std::net::IpAddr::V4(a) => a.octets().to_vec(),
        std::net::IpAddr::V6(a) => a.octets().to_vec(),
    };

    if token.len() < addr.len() || &token[..addr.len()] != addr.as_slice() {
        return None;
    }

    Some(quiche::ConnectionId::from_ref(&token[addr.len()..]))
}
