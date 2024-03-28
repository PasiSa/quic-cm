#[macro_use]
extern crate log;

use crate::manager::start_manager;


fn main() {
    env_logger::builder().format_timestamp_nanos().init();
    start_manager();
}

mod common;
pub mod fifo;
mod manager;
