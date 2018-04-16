use super::*;
use std::thread;

#[derive(Debug)]
pub struct Threadless {
	stream: TcpStream,
	buf: Vec<u8>,
	buf_occupancy: usize,
	payload_bytes: Option<u32>,

	duration: time::Duration,
	events: Events,
	poll: Poll,
}


impl Threadless {
	const TOK: Token = Token(42);
	const NUM_EVENTS: usize = 64;
	const LEN_BYTES: usize = 4; 

	#[inline]
	fn trivial_poll(&mut self) {
		self.poll.poll(
			&mut self.events,
			Some(self.duration)
		).expect("poll failed!");
	}

	fn ensure_buf_capacity(&mut self, capacity: usize) {
		self.buf.resize(capacity, 0u8);
	}

	pub fn current_buffer_size(&mut self) -> usize {
		self.buf_occupancy
	}

	pub fn squash_buffer(&mut self) -> usize {
		self.buf.resize(self.buf_occupancy, 0u8);
		self.buf.shrink_to_fit();
		self.buf_occupancy
	}

	fn inner_try_recv<M: Message>(&mut self) -> Result<M, TryRecvError> {
		if self.payload_bytes.is_none() {
			self.ensure_buf_capacity(Self::LEN_BYTES);
			self.buf_occupancy +=
				self.stream.read(&mut self.buf[self.buf_occupancy..Self::LEN_BYTES])?;
			if self.buf_occupancy == 4 {
				self.payload_bytes = Some(
					(&self.buf[..Self::LEN_BYTES]).read_u32::<LittleEndian>()?
				);
			}
		}
		if let Some(pb) = self.payload_bytes {
			// try to get the payload bytes
			let buf_end: usize = Self::LEN_BYTES + pb as usize;
			self.ensure_buf_capacity(buf_end);
			self.buf_occupancy +=
				self.stream.read(&mut self.buf[Self::LEN_BYTES..buf_end])?;

			if self.buf_occupancy == buf_end {
				// read message to completion!
				let decoded: M = bincode::deserialize(
					&self.buf[Self::LEN_BYTES..buf_end]
				)?;
				self.buf_occupancy = 0;
				self.payload_bytes = None;
				return Ok(decoded);
			}
		}
		Err(TryRecvError::ReadNotReady)
	}
}

impl Middleman for Threadless {
	fn into_inner(mut self) -> (TcpStream, Vec<u8>) {
		self.squash_buffer();
		self.buf.shrink_to_fit();
		(self.stream, self.buf)
	}

	fn new(stream: TcpStream) -> Threadless {
		let poll = Poll::new().unwrap();
		poll.register(&stream, Self::TOK, Ready::writable() | Ready::readable(),
		              PollOpt::edge()).expect("register failed");
		let mut events = Events::with_capacity(Self::NUM_EVENTS);
		poll.poll(&mut events, None).expect("poll failed!");

		for event in events.iter() {} // TODO is this bad????
		
		Self {
			stream: stream,
			buf: vec![],
			buf_occupancy: 0,
			payload_bytes: None,

			duration: time::Duration::from_millis(0),
			poll: poll,
			events: events,
		}
	}

	fn send<M: Message>(&mut self, m: &M) -> Result<(), FatalError> {
		self.trivial_poll();
		let encoded: Vec<u8> = bincode::serialize(&m)?;
		let len = encoded.len();
		if len > ::std::u32::MAX as usize {
			panic!("`send()` can only handle payloads up to std::u32::MAX");
		}
		let mut encoded_len = vec![];
		encoded_len.write_u32::<LittleEndian>(len as u32)?;
		self.stream.write(&encoded_len)?;
		let mut sent_bytes = 0;
		while sent_bytes < len {
			// first try existing events
			for event in self.events.iter() {
				if event.readiness().is_writable() {
					match self.stream.write(&encoded[sent_bytes..]) {
						Err(ref e) if e.kind() == ErrorKind::WouldBlock => (),
						Ok(b) => sent_bytes += b,
						Err(e) => return Err(FatalError::Io(e)),
					}
				}
			}
			self.poll.poll(&mut self.events, None).expect("poll failed!");
		}
		Ok(())
	}

	fn send_all<'m, I, M>(&'m mut self, m_iter: I) -> (usize, Result<(), FatalError>)
	where 
		M: Message + 'm,
		I: Iterator<Item = &'m M>,
	{
		let mut tot_sent = 0;
	    for m in m_iter {
	    	match self.send(m) {
	    		Ok(_) => tot_sent += 1,
	    		Err(FatalError::Io(ref e)) if e.kind() == ErrorKind::WouldBlock => unreachable!(),
	    		Err(e) => return (tot_sent, Err(e)),
	    	}
	    }
	    (tot_sent, Ok(()))
	}

	fn recv<M: Message>(&mut self) -> Result<M, FatalError> {
		// self.trivial_poll();
		if let Ok(msg) = self.inner_try_recv() {
			return Ok(msg);
		}
		let e = &mut self.events as *mut Events;
		unsafe {
			let e = &mut *e; // trivial interior mutability
			loop {
				for event in e.iter() {
					if event.readiness().is_readable() {
						match self.inner_try_recv() {
							Err(TryRecvError::ReadNotReady) => 	(), //spurious
							Ok(msg) => 							return Ok(msg),
							Err(TryRecvError::Fatal(e)) => 		return Err(e),
						}
					}
				}
				self.poll.poll(e, None).expect("poll failed!");
			}
		}
	}

	fn try_recv<M: Message>(&mut self) -> Result<M, TryRecvError> {
		self.trivial_poll();
		self.inner_try_recv()
	}
}
