#[macro_use]
extern crate log;

use crate::manager::start_manager;


fn main() {
    env_logger::builder().format_timestamp_nanos().init();
    start_manager();
}

mod client;
mod connection;
mod manager;
mod mio_tokens;
