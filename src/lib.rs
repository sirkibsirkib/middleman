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
pub use structs::Middleman;

pub mod structs2;

////////////////////// TESTS ////////////////////

// #[cfg(test)]
// mod tests;

#[cfg(test)]
mod tests2;

