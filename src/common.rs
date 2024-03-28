use mio::Token;

pub const QCM_CONTROL_FIFO: &str = "/tmp/qcm-control";
pub const QCM_CLIENT_FIFO: &str = "/tmp/qcm-client";
pub const MIO_QCM_CONTROL: Token = Token(1);
