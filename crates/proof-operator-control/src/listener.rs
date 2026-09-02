use std::{
    future::Future,
    net::{IpAddr, Ipv4Addr, SocketAddr},
};

use axum::Router;

use crate::ControlShellError;

/// Non-secret exact endpoint derived only from the bound socket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopbackOrigin {
    address: SocketAddr,
    host: String,
    origin: String,
}

impl LoopbackOrigin {
    pub fn address(&self) -> SocketAddr {
        self.address
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn origin(&self) -> &str {
        &self.origin
    }

    #[cfg(test)]
    pub(crate) fn for_test(port: u16) -> Self {
        let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
        let host = address.to_string();
        Self {
            address,
            origin: format!("http://{host}"),
            host,
        }
    }
}

/// An already-bound, exact IPv4-loopback listener with an OS-selected port.
pub struct LoopbackListener {
    listener: tokio::net::TcpListener,
    address: SocketAddr,
}

impl LoopbackListener {
    /// Binds exactly `127.0.0.1:0`; there is no fallback or caller-selected address.
    pub async fn bind() -> Result<Self, ControlShellError> {
        Self::bind_address(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)).await
    }

    async fn bind_address(address: SocketAddr) -> Result<Self, ControlShellError> {
        if address.ip() != IpAddr::V4(Ipv4Addr::LOCALHOST) {
            return Err(ControlShellError::ListenerUnavailable);
        }
        let listener = tokio::net::TcpListener::bind(address)
            .await
            .map_err(|_| ControlShellError::ListenerUnavailable)?;
        let address = listener
            .local_addr()
            .map_err(|_| ControlShellError::ListenerUnavailable)?;
        if address.ip() != IpAddr::V4(Ipv4Addr::LOCALHOST) || address.port() == 0 {
            return Err(ControlShellError::ListenerUnavailable);
        }
        Ok(Self { listener, address })
    }

    #[cfg(test)]
    pub(crate) async fn bind_for_test(address: SocketAddr) -> Result<Self, ControlShellError> {
        Self::bind_address(address).await
    }

    /// Returns the only clean, non-secret URL that the launcher may print.
    pub fn clean_url(&self) -> String {
        format!("http://{}/", self.address)
    }

    /// Returns the Host and origin values derived from the actual OS-selected
    /// listener address. Callers cannot provide or override either value.
    pub fn origin(&self) -> LoopbackOrigin {
        let host = self.address.to_string();
        LoopbackOrigin {
            address: self.address,
            origin: format!("http://{host}"),
            host,
        }
    }

    /// Serves directly over HTTP/1.1 until the supplied shutdown future resolves.
    pub async fn serve_until<F>(self, router: Router, shutdown: F) -> Result<(), ControlShellError>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        axum::serve(
            self.listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(shutdown)
        .await
        .map_err(|_| ControlShellError::ListenerUnavailable)
    }
}
