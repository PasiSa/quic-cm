use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::path::Path;
use std::fs::{File, OpenOptions};

use mio::Token;
use nix::sys::stat;
use nix::unistd::mkfifo;


pub struct Fifo {
    //pathname: String,  // this may become into use later
    mio_token: Token,
    file: File,
}

// TODO: remove unwrap()

impl Fifo {
    pub fn new(pathname: &str, mio_token: Token) -> Fifo {
        let fifo_path = Path::new(pathname);
        match mkfifo(fifo_path, stat::Mode::S_IRWXU) {
            Ok(_) => debug!("Named pipe created at {}", pathname),
            Err(e) => debug!("Failed to create named pipe: {}", e),
        }
        Fifo::connect(pathname, mio_token)
    }


    pub fn connect(pathname: &str, mio_token: Token) -> Fifo {
        debug!("Connecting FIFO {}", pathname);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(pathname).unwrap();

        Fifo { mio_token, file }
    }


    pub fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.file.read(buf)
    }


    pub fn write(&self, str: String) -> io::Result<()> {
        let mut fifo = &self.file;
        writeln!(fifo, "{}", str)?;
        Ok(())
    }

    pub fn get_fd(&self) -> i32 {
        self.file.as_raw_fd()
    }


    pub fn get_token(&self) -> Token {
        self.mio_token
    }
}
