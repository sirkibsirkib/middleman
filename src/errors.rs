use super::*;

use std::{
	convert::From,
	boxed::Box,
	io,
};

#[derive(Debug)]
/// The error returned from send-like calls for the Middleman struct.
/// Most errors originate from other dependencies. This enum thus delegates
/// these cases, each to its own variant. In addition TooBigToRepresent is
/// returned when the user passes a structure whose representation requires a length-field
/// larger than std::u32::MAX, which the Middleman is not prepared to represent.
pub enum SendError {
	Io(io::Error),
	TooBigToRepresent,
	Bincode(Box<bincode::ErrorKind>),
}

/// This error is returned from recv-like calls for the Middleman struct.
/// Most errors originate from other dependencies. This enum thus delegates
/// these cases, each to its own variant.
#[derive(Debug)]
pub enum RecvError {
	Io(io::Error),
	Bincode(Box<bincode::ErrorKind>),
}

/////////////////////////////////////////////////////////

impl From<io::Error> for RecvError {
	fn from(e: io::Error) -> Self {
		RecvError::Io(e)
	}
}
impl From<io::Error> for SendError {
	fn from(e: io::Error) -> Self {
		SendError::Io(e)
	}
}
impl From<Box<bincode::ErrorKind>> for RecvError {
	fn from(e: Box<bincode::ErrorKind>) -> Self {
		RecvError::Bincode(e)
	}
}
impl From<Box<bincode::ErrorKind>> for SendError {
	fn from(e: Box<bincode::ErrorKind>) -> Self {
		SendError::Bincode(e)
	}
}