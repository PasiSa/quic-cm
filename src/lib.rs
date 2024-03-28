#[macro_use]
extern crate log;

use mio::Token;

use crate::common::{QCM_CONTROL_FIFO, QCM_CLIENT_FIFO};
use crate::fifo::Fifo;

/// Initiate QUIC connection to given address
pub fn connect(address: &str) -> Result<(), String> {
    let pid = std::process::id();
    let fifoname = format!("{}-{}", QCM_CLIENT_FIFO, pid);
    debug!("Opening FIFO named: {}", fifoname);
    let client_fifo = Fifo::new(fifoname.as_str(), Token(2));
    if client_fifo.is_err() {
        return Err(format!("Creating client fifo failed: {}", client_fifo.err().unwrap()));
    }

    let control_fifo = Fifo::connect(QCM_CONTROL_FIFO, Token(1));
    if control_fifo.is_err() {
        return Err(format!("Connecting control fifo failed: {}", control_fifo.err().unwrap()));
    }

    match control_fifo.unwrap().write(format!("CONN {} {}", address, pid)) {
        Ok(_) => { },
        Err(e) => {
            return Err(format!("Control message sending failed: {}", e))
        },
    }

    let mut buf = [0; 65535];
    match client_fifo.unwrap().read(&mut buf) {
        Ok(_) => {
            debug!("Read: {}", std::str::from_utf8(&buf).unwrap());
            Ok(())
        },
        Err(e) => {
            return Err(format!("Reading control response failed: {}", e))
        }
    }
}

mod common;
pub mod fifo;
