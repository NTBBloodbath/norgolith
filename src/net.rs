use std::net::TcpListener;

/// Check if the given port is available or busy
pub fn is_port_available(port: u16) -> bool {
    TcpListener::bind(("127.0.0.1", port)).is_ok()
}
