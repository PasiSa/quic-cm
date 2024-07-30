use std::{
    fs::remove_file,
    process::{Command, Child},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
    thread,
};

use tokio::time::sleep;
use quic_cm::QuicClient;

mod server;
use crate::server::server;

async fn start_manager() -> Child {
    Command::new("cargo")
        .args(&["run", "--manifest-path", "../quic-cm-manager/Cargo.toml"])
        .spawn()
        .expect("failed to start server")
}


async fn stop_manager(mut manager: Child) {
    manager.kill().expect("failed to kill server");
    manager.wait().expect("failed to wait on server");
}


#[tokio::test]
async fn test_connection() {
    let terminate_signal = Arc::new(AtomicBool::new(false));
    let terminate_signal_clone = terminate_signal.clone();

    // Ensure that the Unix socket does not exist at the start of the test.
    let _ = remove_file("/tmp/qcm-control");

    let server = thread::spawn(|| {
        server(terminate_signal_clone);
    });

    let manager = start_manager().await;
    sleep(Duration::from_secs(1)).await;

    let client = QuicClient::connect("127.0.0.1:7878", "test").await;
    assert!(client.is_ok());
    let client2 = QuicClient::connect("127.0.0.1:7878", "test2").await;
    assert!(client2.is_err());

    stop_manager(manager).await;
    remove_file("/tmp/qcm-control").unwrap();  // TODO: terminate manager properly by signal

    terminate_signal.store(true, Ordering::SeqCst);
    server.join().expect("Join failed");
}
