use quic_cm::connect;


fn main() {
    env_logger::builder().format_timestamp_nanos().init();
    connect("localhost").unwrap();
}
