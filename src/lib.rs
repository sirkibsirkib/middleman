////////////////////// IMPORTS ////////////////////

#[macro_use] extern crate serde_derive;
extern crate serde;
use serde::{
	Serialize, 
	de::DeserializeOwned,
};
extern crate byteorder;
use byteorder::{
	LittleEndian,
	ReadBytesExt,
	WriteBytesExt,
};

extern crate bincode;

extern crate mio;
use mio::*;
use mio::tcp::TcpStream;

use std::{
	convert::From,
	boxed::Box,
	io::{
		Read,
		Write,
		ErrorKind,
	},
	time,
	io,
};

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


