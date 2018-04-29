use super::*;

use std::{
    convert::From,
    boxed::Box,
    io,
};

#[derive(Debug)]

/// Error enum which arise from failing to send a Message with a Middleman.
pub enum SendError {
    Io(io::Error),
    TooBigToRepresent,
    Bincode(Box<bincode::ErrorKind>),
}

/// Error enum which arise from failing to receive a Message with a Middleman.
#[derive(Debug)]
pub enum RecvError {
    Io(io::Error),
    Bincode(Box<bincode::ErrorKind>),
}


/// Error enum which arise from failing to pack a structure implementing Message
#[derive(Debug)]
pub enum PackingError {
    TooBigToRepresent,
    Bincode(Box<bincode::ErrorKind>),
}

/////////////////////////////////////////////////////////

impl From<PackingError> for SendError {
    fn from(e: PackingError) -> Self {
        match e {
            PackingError::TooBigToRepresent => SendError::TooBigToRepresent,
            PackingError::Bincode(e) => SendError::Bincode(e),
        }
    } 
}

///////////////////////////////////////////////

// io::Error
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

// bincode
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
impl From<Box<bincode::ErrorKind>> for PackingError {
    fn from(e: Box<bincode::ErrorKind>) -> Self {
        PackingError::Bincode(e)
    }
}