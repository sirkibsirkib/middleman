use super::*;

use mio::{
	*,
	event::Evented,
	tcp::TcpStream,
};

use ::std::{
	io,
	io::{
		Read,
		Write,
		ErrorKind,
	},
	time,
};

use byteorder::{
	LittleEndian,
	ReadBytesExt,
	WriteBytesExt,
};

/// Wraps a `mio::TcpStream` object. Implmenents `mio::Evented` so it can be
/// registered for polling. Offers `try_read` and `send` functions primarily
/// with variants for convenience. This structure doesn't involve any extra threading.
///
/// Note that data will only travel over the stream when `read_in` and `write_out`
/// are called. Typically, these calls are agglutinated using the single call `read_write`
/// in the context of a `mio::Event`. See the tests for some examples.
#[derive(Debug)]
pub struct Middleman {
	stream: TcpStream,
	buf: Vec<u8>,
	buf_occupancy: usize,
	payload_bytes: Option<u32>,
	to_send: Vec<u8>,
}

impl Middleman {
	const LEN_BYTES: usize = 4;

	/// Wraps the given `mio::TcpStream` in a new middleman.
	pub fn new(stream: TcpStream) -> Middleman {
		Self {
			stream: stream,
			buf: vec![],
			buf_occupancy: 0,
			payload_bytes: None,
			to_send: vec![],
		}
	}

	/// This call is the bread and butter of reading and writing local changes to the 
	/// middleman's internal state to the TcpStream within. If this function is never called,
	/// no progress will ever be made.
	pub fn read_write(&mut self, event: &Event) -> Result<(), io::Error> {
		if event.readiness().is_readable() {
			self.read_in()?;
		}	
		if event.readiness().is_writable() {
			self.write_out()?;
		}
		Ok(())
	}

	/// Force the middleman to try and write any waiting bytes to the TcpStream.
	/// Instead of blocking when the socket isn't ready, the call returns Ok(0).
	/// Errors in writing to the socket emerge as an Err(_) variant.
	/// 
	/// It is advised to rely on the `read_write` for reading and writing instead.
	pub fn write_out(&mut self) -> Result<usize, io::Error> {
		match self.stream.write(& self.to_send[..]) {
			Err(ref e) if e.kind() == ErrorKind::WouldBlock => Ok(0),
			Ok(bytes_written) => {
				self.to_send.drain(..bytes_written);
				Ok(bytes_written)
			},
			Err(e) => Err(e),
		}
	}

	/// Force the middleman to try and read bytes from the inner TcpStream.
	/// Instead of blocking when the socket isn't ready, the call returns Ok(0).
	/// Errors in reading from the socket emerge as an Err(_) variant.
	/// 
	/// It is advised to rely on the `read_write` for reading and writing instead.
	pub fn read_in(&mut self) -> Result<usize, io::Error> {
		let mut total = 0;
		loop {
			let limit = (self.buf_occupancy + 64) + (self.buf_occupancy);
			if self.buf.len() < limit {
				self.buf.resize(limit, 0u8);
			}
			match self.stream.read(&mut self.buf[self.buf_occupancy..]) {
				Ok(bytes) => {
					self.buf_occupancy += bytes;
					total += bytes;
				},
				Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
					return Ok(total);
				},
				Err(e) => return Err(e),
			};
		}
	}

	/// Serialize the given Message, and store it internally for writing to the socket later.
	/// This call will return Err(_) if serialization of the message fails, or the serialized representation
	/// is too large for the Middleman to represent (Structures are required to have a serialized
	/// representation no larger than `std::u32::MAX`).
	///
	/// Note that each send() call may serialize an entirely different type of structure.
	/// Be careful using this feature, as any receive calls on the other end expecting a _different_ type may
	/// fail to read (at best) or erroniously produce valid (albeit undefined) data silently at worst. 
	pub fn send<M: Message>(&mut self, m: &M) -> Result<(), SendError> {
		let encoded: Vec<u8> = bincode::serialize(&m)?;
		let len = encoded.len();
		if len > ::std::u32::MAX as usize {
			return Err(SendError::TooBigToRepresent);
		}
		let mut encoded_len = vec![];
		encoded_len.write_u32::<LittleEndian>(len as u32)?;
		self.to_send.extend_from_slice(&encoded_len);
		self.to_send.extend_from_slice(&encoded);
		Ok(())
	}

	/// Given some iterator over Message structs, `send`s them each in sequence. 
	/// The return result is a tuple (a, b) where a gives the number of messages sent successfully,
	/// and b returns Err(_) with an error if something goes wrong.
	///
	/// After encountering the first error, the iteration will cease and the offending message
	/// _will not_ be sent.
	pub fn send_all<'m, I, M>(&'m mut self, msg_iter: I) -> (usize, Result<(), SendError>) 
	where 
		M: Message + 'm,
		I: Iterator<Item = &'m M>,
	{
		let mut total = 0;
		for msg in msg_iter {
			match self.send(msg) {
				Ok(_) => total += 1,
				Err(e) => return (total, Err(e)),
			}
		}
		(total, Ok(()))
	}

	fn check_payload(&mut self) {
		if self.payload_bytes.is_none() && self.buf_occupancy >= 4 {
			self.payload_bytes = Some(
				(&self.buf[..Self::LEN_BYTES]).read_u32::<LittleEndian>()
				.expect("reading 4 bytes went wrong?")
			);
		}
	}

	/// Attempt to read one message worth of bytes from the input buffer and discard it
	/// This operation doesn't require knowing the type of the struct. As such, this 
	/// can be used to discard a message if somehow it is unserializable.
	pub fn try_discard(&mut self) -> bool {
		self.check_payload();
		if let Some(pb) = self.payload_bytes {
			let buf_end = pb as usize + 4;
			if self.buf_occupancy >= buf_end {
				self.payload_bytes = None;
				self.buf.drain(0..buf_end);
				self.buf_occupancy -= buf_end;
				return true;
			}
		}
		false
	}


	/// Attempt to dedserialize some data in the receiving buffer into a single complete structure
	/// with the given type `M`. If there is insufficient data at the moment, Ok(None) is returned.
	/// 
	/// As the type is provided by the reader, it is possible for the sent message to be misinterpreted
	/// as a different type. At best, this is detected by a failure in deserialization
	pub fn try_recv<M: Message>(&mut self) -> Result<Option<M>, RecvError> {
		self.check_payload();
		if let Some(pb) = self.payload_bytes {
			let buf_end = pb as usize + 4;
			if self.buf_occupancy >= buf_end {
				let decoded: M = bincode::deserialize(
					&self.buf[Self::LEN_BYTES..buf_end]
				)?;
				self.payload_bytes = None;
				self.buf.drain(0..buf_end);
				self.buf_occupancy -= buf_end;
				return Ok(Some(decoded))
			}
		}
		Ok(None)
	}

	pub fn try_recv_all<M: Message>(&mut self, dest_vector: &mut Vec<M>) -> (usize, Result<(), RecvError>) {
		let mut total = 0;
		loop {
			match self.try_recv::<M>() {
				Ok(None) 		=> return (total, Ok(())),
				Ok(Some(msg)) 	=> { dest_vector.push(msg); total += 1; },
				Err(e)			=> return (total, Err(e)),
			};
		}
	}

	// Err only if something fatal
	// Ok(None) only if timout.is_some()
	pub fn recv_blocking<M: Message>(&mut self,
		                         poll: &Poll,
		                         events: &mut Events,
		                         my_tok: Token,
		                         extra_events: &mut Vec<Event>,
		                         mut timeout: Option<time::Duration>) -> Result<Option<M>, RecvError> {

		if let Some(msg) = self.try_recv::<M>()? {
			// trivial case.
			// message was already sitting in the buffer.
			return Ok(Some(msg));
		}
		let started_at = time::Instant::now();
		let mut res = None;
		self.write_out()?;
		loop {
			for event in events.iter() {
				let tok = event.token();
				if res.is_none() && tok == my_tok {
					// event is relevant!
					self.read_in()?;
					match self.try_recv::<M>() {
						Ok(Some(msg)) => {
							// got a message!
							res = Some(msg);
						},
						Ok(None) => (),
						Err(e) => return Err(e),
					}	
				} else {
					extra_events.push(event);
				}
			}
			if let Some(msg) = res {
				// message ready to go. Exiting loop
				return Ok(Some(msg));
			} else {
				poll.poll(events, timeout).expect("poll() failed inside `recv_blocking()`");
				if let Some(t) = timeout {
					// update remaining timeout
					let since = started_at.elapsed();
					if since >= t {
						// ran out of time
						return Ok(None); 
					}
					timeout = Some(t-since);
				}
			}
		}
	}

	/// 
	pub fn try_recv_all_map<F,M>(&mut self, mut func: F) -> (usize, Result<(), RecvError>)
	where M: Message, F: FnMut(M) + Sized {
		let mut total = 0;
		loop {
			match self.try_recv::<M>() {
				Ok(None) 		=> return (total, Ok(())),
				Ok(Some(msg)) 	=> { total += 1; func(msg) },
				Err(e)			=> return (total, Err(e)),
			};
		}
	}


	pub fn recv_ready(&mut self) -> bool {
		self.check_payload();
		self.payload_bytes.is_some()
	}

	pub fn try_recv_bytes(&mut self, dest_buffer: &mut Vec) -> Option<u32> {
		self.check_payload();
		if let Some(pb) = self.payload_bytes {
			let buf_end = pb as usize + 4;
			if self.buf_occupancy >= buf_end {
				dest_buffer.extend_from_slice(
					&self.buf[Self::LEN_BYTES..buf_end]
				);
				self.payload_bytes = None;
				self.buf.drain(0..buf_end);
				self.buf_occupancy -= buf_end;
				Some(pb)
			}
		}
		None
	}


	pub fn try_peek<M: Message>(&mut self) -> Result<Option<M>, RecvError> {
		self.check_payload();
		if let Some(pb) = self.payload_bytes {
			let buf_end = pb as usize + 4;
			if self.buf_occupancy >= buf_end {
				let decoded: M = bincode::deserialize(
					&self.buf[Self::LEN_BYTES..buf_end]
				)?;
				return Ok(Some(decoded))
			}
		}
		Ok(None)
	}

}


impl Evented for Middleman	 {
    fn register(&self, poll: &Poll, token: Token, interest: Ready, opts: PollOpt)
        -> io::Result<()> {
        self.stream.register(poll, token, interest, opts)
    }

    fn reregister(&self, poll: &Poll, token: Token, interest: Ready, opts: PollOpt)
        -> io::Result<()> {
        self.stream.reregister(poll, token, interest, opts)
    }

    fn deregister(&self, poll: &Poll) -> io::Result<()> {
        self.stream.deregister(poll)
    }
}