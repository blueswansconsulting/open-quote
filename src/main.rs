use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::process::ExitCode;
use std::time::Duration;

const DEFAULT_PORT: u16 = 8790;

fn port() -> Result<u16, String> {
    match std::env::var("PORT") {
        Err(_) => Ok(DEFAULT_PORT),
        Ok(p) => p
            .trim()
            .parse::<u16>()
            .ok()
            .filter(|p| *p != 0)
            .ok_or_else(|| format!("PORT must be a number from 1 to 65535, got {p:?}")),
    }
}

/// `open-quote healthcheck`: exit 0 if the local server answers /healthz.
/// Used by the Docker HEALTHCHECK, since the runtime image has no shell or curl.
fn healthcheck(port: u16) -> ExitCode {
    let check = || -> std::io::Result<bool> {
        let mut stream =
            TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_secs(2))?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        stream
            .write_all(b"GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")?;
        let mut buf = [0u8; 64];
        let n = stream.read(&mut buf)?;
        Ok(buf[..n].starts_with(b"HTTP/1.1 200"))
    };
    match check() {
        Ok(true) => ExitCode::SUCCESS,
        _ => ExitCode::FAILURE,
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = term => {} }
}

#[tokio::main]
async fn main() -> ExitCode {
    let port = match port() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("open-quote: {e}");
            return ExitCode::from(2);
        }
    };
    match std::env::args().nth(1).as_deref() {
        Some("healthcheck") => return healthcheck(port),
        Some("--version" | "-V") => {
            println!("open-quote {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Some(other) => {
            eprintln!("open-quote: unknown argument {other:?} (try: healthcheck, --version)");
            return ExitCode::from(2);
        }
        None => {}
    }

    let host = std::env::var("HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let host = host
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_string();
    let addrs: Vec<_> = match (host.as_str(), port).to_socket_addrs() {
        Ok(a) => a.collect(),
        Err(e) => {
            eprintln!("open-quote: HOST {host:?} is not a valid address: {e}");
            return ExitCode::from(2);
        }
    };
    let listener = match tokio::net::TcpListener::bind(addrs.as_slice()).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("open-quote: could not listen on {host}:{port}: {e}");
            return ExitCode::from(1);
        }
    };
    let local = listener
        .local_addr()
        .map(|a| a.to_string())
        .unwrap_or_default();
    println!(
        "Open Quote {} running at http://{local}",
        env!("CARGO_PKG_VERSION")
    );
    if let Err(e) = axum::serve(
        listener,
        open_quote::app().into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    {
        eprintln!("open-quote: server error: {e}");
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}
