# QUIC connection manager

QUIC connection manager allows separate processes that share the same
destination to join same QUIC connection, using different streams within the
connection. Typical use case could be a command line tool such as _ssh_, where
one often has multiple sessions open to the same server, or file transfers using
tools such as _curl_. This way these different instances to same destination can
share the same connection context, particularly congestion control state, and do
not need separate handshake every time.

This repository contains "quic-cm-manager", that is a manager process intended
to run in the background running QUIC client implementation (we use Quiche), and
listening for connection requests from application clients. If application
requests a connection to a destination for which there were no existing active
streams, a new connection is established. If there was an existing connection to
the same destination, only a new stream is added to existing connection.

In addition there is "quic-cm-lib", that build "quic-cm" crate/library that
applications can use to easily access the manager, that operates the actual QUIC
connections on behalf of the applications. The communication between
applications and the manager happens using Unix stream sockets.

## Instructions

You can start the manager simply by `cargo run`. Then application can start a
new connection using QuicClient::connect, from the quic-cm-lib crate. See
`quic-cm-lib/src/bin/testclient.rs` for simple example.
