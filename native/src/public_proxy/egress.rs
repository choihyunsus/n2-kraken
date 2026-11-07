// Public-browser destinations are validated once and connected by the resulting IP.
// Conservative exclusions follow IANA's IPv4/IPv6 special-purpose registries:
// https://www.iana.org/assignments/iana-ipv4-special-registry
// https://www.iana.org/assignments/iana-ipv6-special-registry
use std::io;
use std::net::{IpAddr, SocketAddr};
pub(super) fn denied() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "Browser network request denied",
    )
}

pub(super) fn public_ip(ip: IpAddr) -> bool {
    crate::destination::public_ip(ip)
}

pub(super) fn parse_url(input: &str) -> io::Result<url::Url> {
    if input.len() > 8192 {
        return Err(denied());
    }
    let parsed = url::Url::parse(input).map_err(|_| denied())?;
    if !matches!(parsed.scheme(), "http" | "https")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.host_str().is_none()
        || parsed.port_or_known_default() == Some(0)
    {
        return Err(denied());
    }
    Ok(parsed)
}

pub(super) fn validate_addresses(addresses: &[SocketAddr]) -> io::Result<()> {
    if addresses.is_empty()
        || addresses
            .iter()
            .any(|address| !public_ip(address.ip()) || address.port() == 0)
    {
        return Err(denied());
    }
    Ok(())
}

pub(super) async fn resolve(url: &url::Url) -> io::Result<Vec<SocketAddr>> {
    crate::destination::resolve(url.as_str())
        .await
        .map(|(_, addresses)| addresses)
        .map_err(|_| denied())
}

pub(super) async fn connect(addresses: &[SocketAddr]) -> io::Result<tokio::net::TcpStream> {
    validate_addresses(addresses)?;
    for address in addresses.iter().take(8) {
        if let Ok(Ok(stream)) = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            tokio::net::TcpStream::connect(address),
        )
        .await
        {
            return Ok(stream);
        }
    }
    Err(denied())
}
