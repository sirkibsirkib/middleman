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

	pub fn new(stream: TcpStream) -> Middleman {
		Self {
			stream: stream,
			buf: vec![],
			buf_occupancy: 0,
			payload_bytes: None,
			to_send: vec![],
		}
	}

	pub fn do_io(&mut self, event: &Event) -> Result<(), io::Error> {
		if event.readiness().is_readable() {
			self.read_in()?;
		}	
		if event.readiness().is_writable() {
			self.write_out()?;
		}
		Ok(())
	}

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

	pub fn print_state(&self) {
		println!("  buflen {}", self.buf.len());
		println!("  buf_occ {}", self.buf_occupancy);
		println!("pb {:?}", self.payload_bytes);
		println!("tosend len {:?}", self.to_send.len());
	}

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

	pub fn send<M: Message>(&mut self, m: &M) -> Result<(), FatalError> {
		let encoded: Vec<u8> = bincode::serialize(&m)?;
		let len = encoded.len();
		if len > ::std::u32::MAX as usize {
			return Err(FatalError::TooBigToRepresent);
		}
		let mut encoded_len = vec![];
		encoded_len.write_u32::<LittleEndian>(len as u32)?;
		self.to_send.extend_from_slice(&encoded_len);
		self.to_send.extend_from_slice(&encoded);
		Ok(())
	}


	pub fn send_all<'m, I, M>(&'m mut self, msg_iter: I) -> (usize, Result<(), FatalError>) 
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

	pub fn try_recv<M: Message>(&mut self) -> Result<Option<M>, FatalError> {
		if self.payload_bytes.is_none() && self.buf_occupancy >= 4 {
			self.payload_bytes = Some(
				(&self.buf[..Self::LEN_BYTES]).read_u32::<LittleEndian>()?
			);
		}
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

	pub fn try_recv_all<M: Message>(&mut self, dest_vector: &mut Vec<M>) -> (usize, Result<(), FatalError>) {
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
		                         mut timeout: Option<time::Duration>) -> Result<Option<M>, FatalError> {

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