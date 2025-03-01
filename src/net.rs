use std::net::TcpListener;

/// Check if the given port is available or busy
pub fn is_port_available(port: u16) -> bool {
    tracing::debug!("Checking if port {} is available", port);
    TcpListener::bind(("127.0.0.1", port)).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[cfg_attr(feature = "ci", ignore)]
    async fn test_is_port_available_unused_port() {
        assert!(is_port_available(3333));
    }

    #[tokio::test]
    #[cfg_attr(feature = "ci", ignore)]
    async fn test_is_port_available_used_port() {
        let listener = TcpListener::bind(("127.0.0.1", 8080)).unwrap();
        let port = listener.local_addr().unwrap().port();

        assert!(!is_port_available(port));
    }
}
