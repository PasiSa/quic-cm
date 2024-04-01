use std::net::ToSocketAddrs;

use mio::{Interest, Poll, Token};
use mio::net::UdpSocket;
use quic_cm_lib::fifo::Fifo;
use ring::rand::*;
use quiche::Config;

use crate::mio_tokens::TokenManager;

const MAX_DATAGRAM_SIZE: usize = 1350;

pub enum State {
    Connecting,
    Established,
    Closed,
}

pub struct Connection {
    socket: UdpSocket,
    mio_token: Token,
    qconn: quiche::Connection,
    state: State,
}

impl Connection {

    pub fn new(address: &str, tokenmanager: &mut TokenManager, poll: &mut Poll) -> Connection {
        // TODO: get rid of unwrap and iterate all addresses properly
        let addr = address.to_socket_addrs().unwrap().next().unwrap();
        let bind_addr = match addr {
            std::net::SocketAddr::V4(_) => "0.0.0.0:0",
            std::net::SocketAddr::V6(_) => "[::]:0",
        };
        let mut socket = UdpSocket::bind(bind_addr.parse().unwrap()).unwrap();
        let local_addr = socket.local_addr().unwrap();

        let mut scid = [0; quiche::MAX_CONN_ID_LEN];
        SystemRandom::new().fill(&mut scid[..]).unwrap();
        let scid = quiche::ConnectionId::from_ref(&scid);

        let mut config = set_quic_config();

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

        Connection{
            socket: socket,
            mio_token: token,
            qconn: conn,
            state: State::Connecting,
        }
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

        if self.qconn.is_established() {
            self.state = State::Established;
            self.handle_established();
        }
    }


    pub fn send_data(&mut self) {
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


    pub fn send_from_fifo(&mut self, fifo: &mut Fifo) -> Result<usize, String> {
        let mut buf = [0; 65535];
        let n = match fifo.read(&mut buf) {
            Ok(n) => n,
            Err(e) => {
                error!("Read from fifo failed: {}", e);
                return Err(format!("Read from fifo failed: {}", e));
            },
        };
        // TODO: check that the FIFO command is indeed "SEND". Develop decent parser
        // function for commands.
        if n < 4 {
            return Err(format!("Too short message from Fifo: {} bytes", n));
        }
        let written = match self.qconn.stream_send(4, &buf[4..n], false) {
            Ok(n) => n,
            Err(quiche::Error::Done) => 0,
            Err(e) => {
                return Err(format!("{} stream send failed {:?}", self.qconn.trace_id(), e));
            },
        };
        debug!("send_from_fifo wrote {} bytes", written);
        Ok(written)
    }


    pub fn get_token(&self) -> Token {
        self.mio_token
    }


    pub fn get_state(&self) -> &State {
        &self.state
    }


    fn handle_established(&mut self) {
        let mut buf = [0; 65535];
    
        // Process all readable streams.
        for s in self.qconn.readable() {
            while let Ok((read, fin)) =
                self.qconn.stream_recv(s, &mut buf)
            {
                debug!(
                    "{} received {} bytes",
                    self.qconn.trace_id(),
                    read
                );
    
                let stream_buf = &buf[..read];
    
                debug!(
                    "{} stream {} has {} bytes (fin? {})",
                    self.qconn.trace_id(),
                    s,
                    stream_buf.len(),
                    fin
                );

                let str = String::from_utf8(stream_buf.to_vec()).unwrap();
                debug!("from stream {}: ", s);
                print!("{}", str);
            }
        }
    }
}


fn hex_dump(buf: &[u8]) -> String {
    let vec: Vec<String> = buf.iter().map(|b| format!("{b:02x}")).collect();

    vec.join("")
}


fn set_quic_config() -> Config {
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
