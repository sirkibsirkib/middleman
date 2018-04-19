////////////////////// IMPORTS ////////////////////

extern crate serde;

extern crate byteorder;

extern crate bincode;

extern crate mio;
// use mio::*;
// use mio::tcp::TcpStream;


////////////////////// API ////////////////////

mod errors;
pub use errors::{
	TryRecvError,
	FatalError
};

mod traits;
pub use traits::{
	Middleman,
	Message,
};

mod threadless;
pub use threadless::Threadless;

////////////////////// TESTS ////////////////////

#[cfg(test)]
mod tests;


