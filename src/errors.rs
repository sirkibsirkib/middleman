use super::*;
use ::std::io::ErrorKind;
use std::{
	convert::From,
	boxed::Box,
	io,
};

#[derive(Debug)]
pub enum TryRecvError {
	Fatal(FatalError),
	ReadNotReady,
}
#[derive(Debug)]
pub enum FatalError {
	TooBigToRepresent,
	Io(io::Error),
	Bincode(Box<bincode::ErrorKind>),
}

impl From<io::Error> for TryRecvError {
	fn from(e: io::Error) -> Self {
		if e.kind() == ErrorKind::WouldBlock {
			TryRecvError::ReadNotReady
		} else {
			TryRecvError::Fatal(FatalError::Io(e))
		}
	}
}
impl From<io::Error> for FatalError {
	fn from(e: io::Error) -> Self {
		FatalError::Io(e)
	}
}
impl From<Box<bincode::ErrorKind>> for TryRecvError {
	fn from(e: Box<bincode::ErrorKind>) -> Self {
		TryRecvError::Fatal(FatalError::Bincode(e))
	}
}
impl From<Box<bincode::ErrorKind>> for FatalError {
	fn from(e: Box<bincode::ErrorKind>) -> Self {
		FatalError::Bincode(e)
	}
}