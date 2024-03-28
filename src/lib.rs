#[macro_use]
extern crate log;

use mio::Token;

use crate::common::{QCM_CONTROL_FIFO, QCM_CLIENT_FIFO, MIO_QCM_CONTROL};
use crate::fifo::Fifo;

/// Initiate QUIC connection to given address
pub fn connect(address: &str) {
    let pid = std::process::id();
    let fifoname = format!("{}-{}", QCM_CLIENT_FIFO, pid);
    debug!("Opening FIFO named: {}", fifoname);
    let mut client_fifo = Fifo::new(fifoname.as_str(), Token(2));

    let control_fifo = Fifo::connect(QCM_CONTROL_FIFO, MIO_QCM_CONTROL);
    control_fifo.write(format!("CONN {} {}", address, pid)).unwrap();

    let mut buf = [0; 65535];
    client_fifo.read(&mut buf).unwrap();
    debug!("Read: {}", std::str::from_utf8(&buf).unwrap());
}

mod common;
pub mod fifo;
