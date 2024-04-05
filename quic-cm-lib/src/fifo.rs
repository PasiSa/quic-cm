use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::path::Path;
use std::fs::{self, File, OpenOptions};
use std::str::FromStr;

use mio::Token;
use nix::sys::stat;
use nix::unistd::mkfifo;


pub struct Fifo {
    pathname: String,
    mio_token: Token,
    file: File,
}

impl Fifo {

    /// Create a new FIFO with name 'pathname' and open it for use.
    /// mio_token is the token associated for this FIFO in Mio event processing.
    pub fn new(pathname: &str, mio_token: Token) -> Result<Fifo, String> {
        let fifo_path = Path::new(pathname);
        match mkfifo(fifo_path, stat::Mode::S_IRWXU) {
            Ok(_) => debug!("Named pipe created at {}", pathname),
            Err(e) => debug!("Failed to create named pipe: {}", e),
        }
        Fifo::connect(pathname, mio_token)
    }


    /// Open an existing FIFO with name 'pathname' that has been created earlier.
    /// mio_token is the token associated for this FIFO in Mio event processing.
    pub fn connect(pathname: &str, mio_token: Token) -> Result<Fifo, String> {
        debug!("Connecting FIFO {}", pathname);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(pathname);
        match file {
            Ok(file) => {
                Ok(Fifo {
                    // pathname was already used, so it should be always valid.
                    pathname: String::from_str(pathname).unwrap(),
                    mio_token,
                    file,
                })
            },
            Err(e) => Err(format!("Could not open FIFO: {}", e)),
        }
    }


    /// Read bytes from the FIFO into 'buf'.
    pub fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.file.read(buf)
    }


    /// Write string to the FIFO.
    pub fn write(&self, buf: &[u8]) -> io::Result<usize> {
        let mut fifo = &self.file;
        fifo.write(buf)
    }


    /// Write DATA header to Fifo with number of data bytes.
    pub fn write_data_header(&self, length: u32) -> io::Result<usize> {
        let mut header: [u8; 8] = [0; 8];
        // write "DATA" type specified and u32 length information
        header[..4].copy_from_slice("DATA".as_bytes());
        header[4..].copy_from_slice(&length.to_be_bytes());
        self.write(&header)
    }


    /// Removes the FIFO file. Note that the FIFO is unusable after this.
    pub fn cleanup(&self) {
        fs::remove_file(&self.pathname).unwrap();
    }


    /// Get pathname.
    pub fn get_pathname(&self) -> &String {
        &self.pathname
    }


    /// Get file descriptor correspoding the FIFO.
    pub fn get_fd(&self) -> i32 {
        self.file.as_raw_fd()
    }


    /// Get mio Token for the FIFO.
    pub fn get_token(&self) -> Token {
        self.mio_token
    }
}
