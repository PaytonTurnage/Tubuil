//! TCP implementation of the nhanh API

use crate::*;

use async_std::{
    net::*,
    task::{Context, Poll},
};

use futures::{
    sink::SinkExt,
    stream::{
        self, Fuse, FusedStream, LocalBoxStream, StreamExt, TryStreamExt,
    },
    Sink, Stream,
};

use std::{marker::Unpin, pin::Pin};

use tokio_serde::{formats::*, SymmetricallyFramed};
use tokio_util::{codec::*, compat::*};

pub struct TcpServer {
    incoming: Fuse<Incoming<'static>>,
}

impl TcpServer {
    pub async fn bind(addrs: impl ToSocketAddrs) -> Result<TcpServer> {
        let listener = TcpListener::bind(addrs).await?;
        let listener = Box::leak(Box::new(listener));

        Ok(Self {
            incoming: listener.incoming().fuse(),
        })
    }
}

impl FusedStream for TcpServer {
    fn is_terminated(&self) -> bool {
        self.incoming.is_terminated()
    }
}

impl Server<TcpConnection> for TcpServer {}

impl Stream for TcpServer {
    type Item = Result<TcpConnection>;
    fn poll_next(
        mut self: Pin<&mut Self>,
        ctx: &mut Context,
    ) -> Poll<Option<Self::Item>> {
        match Pin::new(&mut self.incoming).poll_next(ctx) {
            Poll::Ready(Some(Ok(tcp_stream))) => {
                let peer_addr = match tcp_stream.peer_addr() {
                    Ok(peer_addr) => peer_addr,
                    Err(e) => return Poll::Ready(Some(Err(e.into()))),
                };
                Poll::Ready(Some(Ok(TcpConnection::from((
                    tcp_stream, peer_addr,
                )))))
            }
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e.into()))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

pub struct TcpConnection {
    receiver: LocalBoxStream<'static, Result<Datagram>>,
    sender:
        Pin<Box<dyn Sink<SendCmd, Error = Box<dyn std::error::Error>> + Unpin>>,
    peer_addr: SocketAddr,
}

impl TcpConnection {
    pub async fn connect(address: impl ToSocketAddrs) -> Result<Self> {
        let tcp_stream = TcpStream::connect(address).await?;
        let peer_addr = tcp_stream.peer_addr()?;
        Ok(TcpConnection::from((tcp_stream, peer_addr)))
    }

    pub fn peer_addr(&self) -> SocketAddr {
        self.peer_addr
    }

    fn send_gate() -> impl FnMut(
        SendCmd,
    ) -> stream::Iter<
        <Option<Result<Datagram>> as IntoIterator>::IntoIter,
    > {
        let mut total_sent = 0;
        move |send_cmd: SendCmd| {
            stream::iter(match send_cmd.delivery_mode {
                DeliveryMode::ReliableOrdered(stream_id) => {
                    total_sent += 1;
                    Some(Ok(Datagram {
                        data: send_cmd.data,
                        stream_position: Some(StreamPosition {
                            stream_id,
                            index: StreamIndex::Ordinal(total_sent),
                        }),
                    }))
                }
                _ => None,
            })
        }
    }
}

impl From<(TcpStream, SocketAddr)> for TcpConnection {
    fn from((stream, peer_addr): (TcpStream, SocketAddr)) -> Self {
        let framer = LengthDelimitedCodec::new();
        let stream = Framed::new(stream.compat(), framer);
        let codec = SymmetricalBincode::default();

        let wire = SymmetricallyFramed::new(stream, codec);
        let wire = wire.sink_map_err(Into::into);
        let wire = wire.map_err(Into::into);
        let (wire_sink, wire_stream) = wire.split();

        let wire_sink = wire_sink.with_flat_map(Box::new(Self::send_gate()));

        Self {
            receiver: wire_stream.boxed_local(),
            sender: Pin::new(Box::new(wire_sink)),
            peer_addr,
        }
    }
}

impl Connection for TcpConnection {}

impl Sink<SendCmd> for TcpConnection {
    type Error = Box<dyn std::error::Error>;
    fn poll_ready(
        mut self: Pin<&mut Self>,
        ctx: &mut Context,
    ) -> Poll<Result<()>> {
        Pin::new(&mut self.sender)
            .poll_ready(ctx)
            .map_err(Into::into)
    }
    fn start_send(mut self: Pin<&mut Self>, item: SendCmd) -> Result<()> {
        Pin::new(&mut self.sender)
            .start_send(item)
            .map_err(Into::into)
    }
    fn poll_flush(
        mut self: Pin<&mut Self>,
        ctx: &mut Context,
    ) -> Poll<Result<()>> {
        Pin::new(&mut self.sender)
            .poll_flush(ctx)
            .map_err(Into::into)
    }
    fn poll_close(
        mut self: Pin<&mut Self>,
        ctx: &mut Context,
    ) -> Poll<Result<()>> {
        Pin::new(&mut self.sender)
            .poll_close(ctx)
            .map_err(Into::into)
    }
}

impl Stream for TcpConnection {
    type Item = Result<Datagram>;
    fn poll_next(
        mut self: Pin<&mut Self>,
        ctx: &mut Context,
    ) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.receiver).poll_next(ctx)
    }
}

impl FusedStream for TcpConnection {
    fn is_terminated(&self) -> bool {
        false
    }
}
