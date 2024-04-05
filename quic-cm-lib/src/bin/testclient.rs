use quic_cm_lib::QuicClient;


fn main() {
    env_logger::builder().format_timestamp_nanos().init();
    let mut client = QuicClient::connect("127.0.0.1:7878").unwrap();
    let bytes = *b"ABCDEF\n";
    client.write(&bytes).unwrap();

    let mut incoming: [u8; 10000] = [0; 10000];
    client.read(&mut incoming).unwrap();
    println!("Received: {}", String::from_utf8(incoming.to_vec()).unwrap())
}
