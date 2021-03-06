use core::convert::TryFrom;
use core::pin::Pin;
use core::task::{Context, Poll};
use futures::channel::oneshot;
use pin_project_lite::pin_project;
use tokio::net::TcpStream;
use tokio_rustls::server::TlsStream;

pub trait OptionHeaderBuilder {
    // Add optional header
    fn option_header<K, V>(self, key: K, value_opt: Option<V>) -> Self
    where
        http::header::HeaderName: TryFrom<K>,
        <http::header::HeaderName as TryFrom<K>>::Error: Into<http::Error>,
        http::header::HeaderValue: TryFrom<V>,
        <http::header::HeaderValue as TryFrom<V>>::Error: Into<http::Error>;
}

impl OptionHeaderBuilder for http::response::Builder {
    // Add optional header
    fn option_header<K, V>(self, key: K, value_opt: Option<V>) -> Self
    where
        http::header::HeaderName: TryFrom<K>,
        <http::header::HeaderName as TryFrom<K>>::Error: Into<http::Error>,
        http::header::HeaderValue: TryFrom<V>,
        <http::header::HeaderValue as TryFrom<V>>::Error: Into<http::Error>,
    {
        if let Some(value) = value_opt {
            self.header(key, value)
        } else {
            self
        }
    }
}

pin_project! {
    pub struct FinishDetectableStream<S> {
        #[pin]
        stream_pin: S,
        finish_notifier: Option<oneshot::Sender<()>>,
    }
}

impl<S: futures::stream::Stream> futures::stream::Stream for FinishDetectableStream<S> {
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut this = self.project();
        match this.stream_pin.as_mut().poll_next(cx) {
            // If body is finished
            Poll::Ready(None) => {
                // Notify finish
                if let Some(notifier) = this.finish_notifier.take() {
                    notifier.send(()).unwrap();
                }
                Poll::Ready(None)
            }
            poll => poll,
        }
    }
}

pub fn finish_detectable_stream<S>(
    stream: S,
) -> (FinishDetectableStream<S>, oneshot::Receiver<()>) {
    let (finish_notifier, finish_waiter) = oneshot::channel::<()>();
    (
        FinishDetectableStream {
            stream_pin: stream,
            finish_notifier: Some(finish_notifier),
        },
        finish_waiter,
    )
}

pub fn make_io_error(err: String) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::Other, err)
}

// (base: https://github.com/ctz/hyper-rustls/blob/5f073724f7b5eee3a2d72f0a86094fc2718b51cd/examples/server.rs)
pub fn load_tls_config(
    cert_path: impl AsRef<std::path::Path>,
    key_path: impl AsRef<std::path::Path> + std::fmt::Display,
) -> std::io::Result<rustls::ServerConfig> {
    // Load public certificate.
    let mut cert_reader = std::io::BufReader::new(std::fs::File::open(cert_path)?);
    let certs = rustls::internal::pemfile::certs(&mut cert_reader)
        .map_err(|_| make_io_error("unable to load certificate".to_owned()))?;
    // Load private key.
    let mut key_reader = std::io::BufReader::new(std::fs::File::open(key_path)?);
    // Load and return a single private key.
    let key = rustls::internal::pemfile::pkcs8_private_keys(&mut key_reader)
        .map_err(|_| make_io_error("unable to load private key".to_owned()))?
        .remove(0);
    // Do not use client certificate authentication.
    let mut cfg = rustls::ServerConfig::new(rustls::NoClientAuth::new());
    // Select a certificate to use.
    cfg.set_single_cert(certs, key).unwrap();
    // Configure ALPN to accept HTTP/2, HTTP/1.1 in that order.
    cfg.set_protocols(&[b"h2".to_vec(), b"http/1.1".to_vec()]);
    Ok(cfg)
}

pub struct HyperAcceptor<S> {
    pub acceptor: core::pin::Pin<Box<S>>,
}

impl<S> hyper::server::accept::Accept for HyperAcceptor<S>
where
    S: futures::stream::Stream<Item = Result<TlsStream<TcpStream>, std::io::Error>>,
{
    type Conn = TlsStream<TcpStream>;
    type Error = std::io::Error;

    fn poll_accept(
        mut self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context,
    ) -> core::task::Poll<Option<Result<Self::Conn, Self::Error>>> {
        self.acceptor.as_mut().poll_next(cx)
    }
}

pin_project! {
    pub struct One<T> {
        value: Option<T>,
    }
}

impl<T> One<T> {
    fn new(x: T) -> Self {
        One { value: Some(x) }
    }
}

impl<T> futures::stream::Stream for One<T> {
    type Item = T;

    fn poll_next(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(self.project().value.take())
    }
}

#[inline]
pub fn one_stream<T>(x: T) -> One<T> {
    One::new(x)
}
