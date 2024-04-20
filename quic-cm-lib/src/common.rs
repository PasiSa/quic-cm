use std::{
    io::{Result, Write},
    os::unix::net,
};

use tokio::net::UnixStream;
use tokio::io::AsyncWriteExt;


pub const QCM_CONTROL_SOCKET: &str = "/tmp/qcm-control";


/// Write DATA header to socket with number of data bytes.
pub async fn write_data_header(socket: &mut UnixStream, length: u32) -> Result<usize> {
    let mut header: [u8; 8] = [0; 8];
    // write "DATA" type specified and u32 length information
    header[..4].copy_from_slice("DATA".as_bytes());
    header[4..].copy_from_slice(&length.to_be_bytes());
    socket.write(&header).await
}


// TODO: stupid temporary solution to make cm-manager work without tokio
pub fn write_data_header_sync(socket: &mut net::UnixStream, length: u32) -> Result<usize> {
    let mut header: [u8; 8] = [0; 8];
    // write "DATA" type specified and u32 length information
    header[..4].copy_from_slice("DATA".as_bytes());
    header[4..].copy_from_slice(&length.to_be_bytes());
    socket.write(&header)
}