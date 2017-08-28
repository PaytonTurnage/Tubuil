//! event defines events, the atomic temporal unit of the miknet protocol.

use gram::{Chunk, Gram};
use timers::Timer;

#[derive(Debug, PartialEq)]
pub enum Api {
    Tx(Vec<u8>),
    Disc,
    Conn,
}

#[derive(Debug, PartialEq)]
pub enum Event {
    Api(Api),
    Gram(Gram),
    Chunk(Chunk),
    Timer(Timer),
    InvalidGram,
}
