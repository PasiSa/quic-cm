use quic_cm::connect;


fn main() {
    env_logger::builder().format_timestamp_nanos().init();
    connect("127.0.0.1:7878").unwrap();
}
