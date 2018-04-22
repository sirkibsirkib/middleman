use super::*;

use std::{
	convert::From,
	boxed::Box,
	io,
};

#[derive(Debug)]
pub enum SendError {
	Io(io::Error),
	TooBigToRepresent,
	Bincode(Box<bincode::ErrorKind>),
}

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