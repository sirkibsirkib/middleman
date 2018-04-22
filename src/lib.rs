////////////////////// IMPORTS ////////////////////

extern crate serde;
extern crate byteorder;
extern crate bincode;
extern crate mio;

#[cfg(test)]
#[macro_use] extern crate serde_derive;

////////////////////// API ////////////////////

mod errors;
pub use errors::{
	TryRecvError,
	FatalError
};

mod traits;
pub use traits::{
	Message,
};

mod structs;
pub use structs::Middleman;

////////////////////// TESTS ////////////////////

#[cfg(test)]
mod tests;


