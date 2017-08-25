//! gram defines the atomic unit of the miknet protocol.

use bincode::{Bounded, deserialize, serialize_into};
use event::Event;
use std::io;
use std::net::SocketAddr;
use tokio_core::net::UdpCodec;

pub const MTU: Bounded = Bounded(1400);
pub const MTU_BYTES: usize = 1400;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Chunk {
    Init { token: u32, tsn: u32 },
    InitAck { token: u32, tsn: u32, state_cookie: u32 },
    CookieEcho(u32),
    CookieAck,
}

impl Into<Event> for Chunk {
    fn into(self) -> Event { Event::Chunk(self) }
}

/// Gram is the atomic unit of the miknet protocol. All transmissions are represented as a gram
/// before they are written on the network.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Gram {
    token: u32,
    chunks: Vec<Chunk>,
}

impl Into<Vec<Event>> for Gram {
    fn into(mut self) -> Vec<Event> {
        let events = self.chunks.drain(0..).map(Chunk::into).collect();
        events
    }
}

/// GramCodec defines the protocol rules for sending grams over udp.
pub struct GramCodec;

impl UdpCodec for GramCodec {
    type In = Option<(SocketAddr, Gram)>;
    type Out = (SocketAddr, Vec<u8>);

    fn decode(&mut self, src: &SocketAddr, buf: &[u8]) -> io::Result<Self::In> {
        match deserialize::<Gram>(buf) {
            Ok(gram) => Ok(Some((*src, gram))),
            Err(_) => Ok(None),
        }
    }

    fn encode(&mut self, msg: Self::Out, buf: &mut Vec<u8>) -> SocketAddr {
        let (dest, mut payload) = msg;
        buf.append(&mut payload);
        dest
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use Result;
    use bincode::serialize;
    use futures::Stream;
    use std::net::{self, SocketAddr};
    use std::str::FromStr;
    use tokio_core::net::UdpSocket;
    use tokio_core::reactor::Core;

    #[test]
    fn runner() {
        let gram = Gram { token: 0, chunks: vec![Chunk::CookieAck] };
        assert_eq!(events(serialize(&gram, MTU).expect("serialized_gram"))
                       .expect("to generate events"),
                   gram);
    }

    fn events(payload: Vec<u8>) -> Result<Gram> {
        let mut core = Core::new()?;
        let handle = core.handle();
        let (sender, receiver) = (net::UdpSocket::bind("127.0.0.1:0")?,
                                  UdpSocket::bind(&SocketAddr::from_str("127.0.0.1:0")?, &handle)?);
        let test_addr = receiver.local_addr()?;

        sender.send_to(&payload, &test_addr)?;
        let product = match core.run(receiver.framed(GramCodec {}).into_future()) {
            Ok((product, _)) => Ok(product),
            Err((e, _)) => Err(e),
        }?;

        match product {
            Some(Some((sender_addr, gram))) => {
                assert_eq!(sender_addr, sender.local_addr()?);
                Ok(gram)
            }
            _ => panic!("no events in the stream!"),
        }
    }
}
