use quic_cm_lib::QuicClient;


fn main() {
    env_logger::builder().format_timestamp_nanos().init();
    let mut client = QuicClient::connect("127.0.0.1:7878").unwrap();
    let bytes = *b"ABCDEF\n";
    client.write(&bytes).unwrap();
}
