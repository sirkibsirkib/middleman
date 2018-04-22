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

	fn check_payload(&mut self) {
		if self.payload_bytes.is_none() && self.buf_occupancy >= 4 {
			self.payload_bytes = Some(
				(&self.buf[..Self::LEN_BYTES]).read_u32::<LittleEndian>()
				.expect("reading 4 bytes went wrong?")
			);
		}
	}

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
	/// as a different type. At best, this is detected by a failure in deserialization. If an error occurs, 
	/// the data is _not_ consumed from the Middleman. Subsequent reads will operate on the same data. 
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

	/// Keep attempting to call `try_recv` until the next message is no longer ready.
	/// Will `push` received messages into the provided destination buffer in the order received.
	/// See `try_recv` for more information.
	/// 
	/// Returns (a, b) where a is the number of messages successfully received and 
	/// b is Err(_) if an error occurs attempting to deserialize a message. Upon the first error,
	/// no further messages _nor_ the offending message are recieved or removed from the buffer. Subsequent
	/// recv() calls will thus operate on the same data that caused the error before.
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

	/// Hijack the mio event loop, reading and writing to the socket as polling allows. Events not related to 
	/// the recv() of this middleman (determined from the provided `mio::Token`) are pushed into the provided
	/// extra_events vector. Returns Ok(Some(_)) if a message was successfully received. May return Ok(None) if
	/// the user provides as `timeout` some non-none Duration. Returns Err(_) if something goes wrong with reading 
	/// from the socket or deserializing the message. See `try_recv` for more information. 
	///
	/// WARNING: The user should take care to iterate over these events also, as without them
	/// all the `Evented` objects registered with the provided poll object might experience lost wakeups.
	/// It is suggested that in the event of any recv_blocking calls in your loop, you extend the event
	/// loop with a drain() on the same vector passed here as `extra_events`
	/// (using the iterator `chain` function, for example.)
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

	/// A convenience function similar to `try_recv_all`, but instead calls a provided function for each
	/// successfully received message instead of pushing them to a provided buffer.
	/// See `try_recv_all` and `try_recv` for moree information
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


	/// Returns true if the internal buffer current holds 1+ messages, ready to be received.
	/// In this case, `try_recv` and similar functions are guaranteed to return a message.
	pub fn recv_ready(&mut self) -> bool {
		self.check_payload();
		self.payload_bytes.is_some()
	}

	/// Attempts to read a message from the internal buffer, just like `try_recv`,
	/// but instead makes no attempt to deserialize the struct data. Instead,
	/// bytes are appended to the provided byte buffer.
	/// For this reason, no type annotation is required.
	pub fn try_recv_bytes(&mut self, dest_buffer: &mut Vec<u8>) -> Option<u32> {
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
				return Some(pb);
			}
		}
		None
	}

	/// Similar to `try_recv`, whether or not the call succeeds, the data is not removed from
	/// the internal buffer. Subsequent calls will thus repeatedely deserialize the same bytes.
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