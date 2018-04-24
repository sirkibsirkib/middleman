////////////////////// IMPORTS ////////////////////

extern crate serde;
extern crate byteorder;
extern crate bincode;
extern crate mio;
// extern crate ringtail;

#[cfg(test)]
#[macro_use] extern crate serde_derive;

////////////////////// API ////////////////////

mod errors;
pub use errors::{
    SendError,
    RecvError,
};

mod traits;
pub use traits::{
    Message,
};

mod structs;
pub use structs::{
	Middleman,
	PackedMessage,
};

////////////////////// TESTS ////////////////////

#[cfg(test)]
mod tests;

