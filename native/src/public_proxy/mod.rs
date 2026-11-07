// One process-owned public network proxy for browser navigation and subresources.
mod egress;
mod protocol;
#[cfg(test)]
mod tests;
use std::io;
use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::OnceCell;

const MAX_CONNECTIONS: usize = 32;
const MAX_TRANSFER: u64 = 64 * 1024 * 1024;
const CONNECTION_LIFETIME: std::time::Duration = std::time::Duration::from_secs(60);
static PROXY: OnceCell<PublicProxy> = OnceCell::const_new();
#[cfg(test)]
pub(crate) static ESTABLISHED: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);
struct PublicProxy {
    address: SocketAddr,
    task: tokio::task::JoinHandle<()>,
}

pub(crate) async fn address() -> io::Result<SocketAddr> {
    Ok(PROXY.get_or_try_init(start).await?.address)
}
pub(crate) fn shutdown() {
    if let Some(proxy) = PROXY.get() {
        proxy.task.abort();
    }
}
pub(crate) async fn validate(url: &str) -> io::Result<()> {
    egress::resolve(&egress::parse_url(url)?).await.map(|_| ())
}
async fn start() -> io::Result<PublicProxy> {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let address = listener.local_addr()?;
    let task = tokio::spawn(async move {
        let mut jobs = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                connection = listener.accept() => {
                    let Ok((stream,_)) = connection else {break;};
                    if jobs.len() < MAX_CONNECTIONS {
                        jobs.spawn(async move {let _ = tokio::time::timeout(CONNECTION_LIFETIME, serve(stream)).await;});
                    }
                }
                _ = jobs.join_next(), if !jobs.is_empty() => {}
            }
        }
    });
    Ok(PublicProxy { address, task })
}

async fn read_header(stream: &mut TcpStream) -> io::Result<(Vec<u8>, Vec<u8>)> {
    let mut buffer = Vec::new();
    let mut chunk = [0; 4096];
    loop {
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            return Err(egress::denied());
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(end) = buffer
            .windows(4)
            .position(|part| part == b"\r\n\r\n")
            .map(|offset| offset + 4)
        {
            if end > protocol::MAX_HEADER {
                return Err(egress::denied());
            }
            let rest = buffer.split_off(end);
            return Ok((buffer, rest));
        }
        if buffer.len() > protocol::MAX_HEADER {
            return Err(egress::denied());
        }
    }
}
async fn prepare(stream: &mut TcpStream) -> io::Result<(TcpStream, protocol::Request, Vec<u8>)> {
    let (header, body) =
        tokio::time::timeout(std::time::Duration::from_secs(5), read_header(stream))
            .await
            .map_err(|_| egress::denied())??;
    let request = protocol::parse(&header)?;
    let upstream = egress::connect(&egress::resolve(&request.url).await?).await?;
    #[cfg(test)]
    ESTABLISHED.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    Ok((upstream, request, body))
}
async fn serve(mut client: TcpStream) -> io::Result<()> {
    let (upstream, request, initial) = match prepare(&mut client).await {
        Ok(value) => value,
        Err(error) => {
            let _ = client.write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 30\r\nConnection: close\r\n\r\nBrowser network request denied").await;
            return Err(error);
        }
    };
    forward(client, upstream, request, initial).await
}
async fn forward(
    mut client: TcpStream,
    mut upstream: TcpStream,
    request: protocol::Request,
    initial: Vec<u8>,
) -> io::Result<()> {
    if request.tunnel {
        client
            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .await?;
    } else {
        upstream.write_all(&request.forwarded).await?;
    }
    upstream.write_all(&initial).await?;
    let (cr, cw) = client.split();
    let (ur, uw) = upstream.split();
    tokio::try_join!(
        copy_limited(cr, uw, MAX_TRANSFER - initial.len() as u64),
        copy_limited(ur, cw, MAX_TRANSFER)
    )?;
    Ok(())
}
async fn copy_limited<R, W>(mut reader: R, mut writer: W, limit: u64) -> io::Result<()>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut bytes = 0;
    let mut buffer = [0; 8192];
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            return writer.shutdown().await;
        }
        bytes += read as u64;
        if bytes > limit {
            return Err(egress::denied());
        }
        writer.write_all(&buffer[..read]).await?;
    }
}
