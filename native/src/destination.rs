// Public retrieval destinations are resolved once and connected using those exact addresses.
// Conservative ranges follow the IANA IPv4 and IPv6 special-purpose registries.
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::sync::{Arc, LazyLock};
use tokio::sync::Semaphore;

static DNS_SLOTS: LazyLock<Arc<Semaphore>> = LazyLock::new(|| Arc::new(Semaphore::new(8)));
const DENIED: &str = "Public retrieval destination rejected";

pub(crate) fn parse(input: &str) -> Result<url::Url, String> {
    if input.len() > 8192 || input.chars().any(char::is_control) {
        return Err(DENIED.into());
    }
    let mut url = url::Url::parse(input).map_err(|_| DENIED)?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.host_str().is_none()
        || url.port_or_known_default() == Some(0)
    {
        return Err(DENIED.into());
    }
    url.set_fragment(None);
    Ok(url)
}

const IPV4_RESERVED: &[([u8; 4], u32)] = &[
    ([0, 0, 0, 0], 8),
    ([10, 0, 0, 0], 8),
    ([100, 64, 0, 0], 10),
    ([127, 0, 0, 0], 8),
    ([169, 254, 0, 0], 16),
    ([172, 16, 0, 0], 12),
    ([192, 168, 0, 0], 16),
    ([192, 0, 0, 0], 24),
    ([192, 0, 2, 0], 24),
    ([192, 88, 99, 0], 24),
    ([198, 18, 0, 0], 15),
    ([198, 51, 100, 0], 24),
    ([203, 0, 113, 0], 24),
    ([224, 0, 0, 0], 3),
];

pub(crate) fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let address = u32::from(ip);
            !IPV4_RESERVED.iter().any(|(network, prefix)| {
                let mask = u32::MAX << (32 - prefix);
                address & mask == u32::from_be_bytes(*network) & mask
            })
        }
        IpAddr::V6(ip) => {
            if let Some(mapped) = ip.to_ipv4_mapped() {
                return public_ip(IpAddr::V4(mapped));
            }
            let s = ip.segments();
            (s[0] & 0xe000) == 0x2000
                && !(s[0] == 0x2001 && (s[1] < 0x0200 || s[1] == 0x0db8))
                && s[0] != 0x2002
                && !(s[0] == 0x3fff && s[1] < 0x1000)
        }
    }
}

pub(crate) fn validate(addresses: &[SocketAddr]) -> Result<(), String> {
    if addresses.is_empty()
        || addresses.len() > 32
        || addresses
            .iter()
            .any(|address| address.port() == 0 || !public_ip(address.ip()))
    {
        return Err(DENIED.into());
    }
    Ok(())
}

pub(crate) async fn resolve(input: &str) -> Result<(url::Url, Vec<SocketAddr>), String> {
    let url = parse(input)?;
    let host = url
        .host_str()
        .ok_or(DENIED)?
        .trim_matches(['[', ']'])
        .to_owned();
    let port = url.port_or_known_default().ok_or(DENIED)?;
    let addresses = if let Ok(ip) = host.parse::<IpAddr>() {
        vec![SocketAddr::new(ip, port)]
    } else {
        let permit = DNS_SLOTS
            .clone()
            .try_acquire_owned()
            .map_err(|_| "DNS capacity reached")?;
        // The resolver retains its permit if the async caller leaves or times out.
        let lookup = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            (host.as_str(), port)
                .to_socket_addrs()
                .map(|addresses| addresses.take(33).collect::<Vec<_>>())
        });
        tokio::time::timeout(std::time::Duration::from_secs(5), lookup)
            .await
            .map_err(|_| "DNS lookup timed out")?
            .map_err(|_| "DNS lookup stopped")?
            .map_err(|_| "DNS lookup failed")?
    };
    validate(&addresses)?;
    Ok((url, addresses))
}
