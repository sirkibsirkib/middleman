#[macro_use] extern crate serde_derive;
extern crate serde;
use serde::{
	Serialize, 
	de::DeserializeOwned,
};
extern crate byteorder;
use byteorder::{
	LittleEndian,
	ReadBytesExt,
	WriteBytesExt,
};

extern crate bincode;

extern crate mio;
use mio::*;
use mio::tcp::{
	TcpListener,
	TcpStream,
};

use std::{
	net::{
		IpAddr,
		Ipv4Addr,
		SocketAddr,
	},
	io::{
		Read,
		Write,
		ErrorKind,
	},
	time,
	io,
	thread,
};


#[cfg(test)]
mod tests;

pub trait Message: Serialize + DeserializeOwned {}

#[derive(Debug)]
pub enum TryRecvError {
	Io(io::Error),
	ReadNotReady,
}
impl std::convert::From<io::Error> for TryRecvError {
	fn from(e: io::Error) -> Self {
		if e.kind() == ErrorKind::WouldBlock {
			TryRecvError::ReadNotReady
		} else {
			TryRecvError::Io(e)
		}
	}
}

pub trait Courier {
	fn try_recv<M: Message>(&mut self) -> Result<M, TryRecvError>;
	fn recv<M: Message>(&mut self) -> Result<M, io::Error>;
	fn send<M: Message>(&mut self, m: &M) -> Result<(), io::Error>;
	fn send_all<'m, I, M>(&'m mut self, m_iter: I) -> (usize, Result<(), io::Error>)
	where 
		M: Message + 'm,
		I: Iterator<Item = &'m M>;
}




///////////////////
#[derive(Debug, Serialize, Deserialize)]
pub struct Msg(pub u32, pub String);
impl Message for Msg {}


#[derive(Debug, Serialize, Deserialize)]
struct Bullshit {
	s: String,
	num: u16,
}
impl Message for Bullshit {}

fn main() {
    let (h, addr) = mio_echoserver();
	let stream = TcpStream::connect(&addr).expect("x");
	stream.set_nodelay(true).is_ok();
    let mut courier = OneThread::new(stream);
    let 	batch = vec![
		Msg(1, "one".to_owned()),
		Msg(2, "two".to_owned()),
		Msg(3, "three".to_owned()),
		Msg(4, "four".to_owned()),
	];
    let went = courier.send_all(batch.iter());
	// let r1 = courier.send(&Msg(1, "one".to_owned()));
	// let r1 = courier.send(&Msg(2, "two".to_owned()));
	// let r1 = courier.send(&Msg(3, "three".to_owned()));
	// let r1 = courier.send(&Msg(4, "four".to_owned()));
	println!("sending of {:#?} went {:?}", &batch, &went);

	let r1 = courier.send(&Bullshit {
		s: "potato".to_owned(),
		num: 21,
	});
	println!("get back {:?}", &courier.recv::<Msg>());
	println!("get back {:?}", &courier.recv::<Msg>());
	println!("get back {:?}", &courier.recv::<Msg>());
	println!("get back {:?}", &courier.recv::<Msg>());
	println!("get back {:?}", &courier.recv::<Bullshit>());
    h.join().is_ok();
}


/*
Totally transparent object
uses a small
*/
#[derive(Debug)]
pub struct OneThread {
	stream: TcpStream,
	buf: Vec<u8>,
	buf_occupancy: usize,
	payload_bytes: Option<u32>,

	duration: time::Duration,
	events: Events,
	poll: Poll,
}

const LEN_BYTES: usize = 4; 

impl OneThread {
	const TOK: Token = Token(42);
	const NUM_EVENTS: usize = 64;

	pub fn new(stream: TcpStream) -> OneThread {
		let poll = Poll::new().unwrap();
		poll.register(&stream, Self::TOK, Ready::writable() | Ready::readable(),
		              PollOpt::edge()).expect("register failed");
		Self {
			stream: stream,
			buf: vec![],
			buf_occupancy: 0,
			payload_bytes: None,

			duration: time::Duration::from_millis(0),
			poll: poll,
			events: Events::with_capacity(Self::NUM_EVENTS),
		}
	}

	#[inline]
	fn trivial_poll(&mut self) {
		println!("Polling self, kek");
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

	fn inner_try_recv<M>(&mut self) -> Result<M, TryRecvError>
	where
		M: Message,
	{
		println!("c try recv fam");
		if self.payload_bytes.is_none() {
			println!("BRANCH A");
			self.ensure_buf_capacity(LEN_BYTES);
			let dank = self.stream.read(&mut self.buf[self.buf_occupancy..LEN_BYTES]);
			println!("dank = {:?}", &dank);
			println!("buf occupancy is {}", self.buf_occupancy);
			let dank = dank?;
			self.buf_occupancy +=
				dank;
			println!("buf is nao {:?}", &self.buf);
			if self.buf_occupancy == 4 {
				self.payload_bytes = Some(
					(&self.buf[0..LEN_BYTES]).read_u32::<LittleEndian>()
					.expect("naimen")
				);
			}
		}
		if let Some(pb) = self.payload_bytes {
			println!("BRANCH B");
			// try to get the payload bytes
			let buf_end: usize = LEN_BYTES + pb as usize;
			self.ensure_buf_capacity(buf_end);
			self.buf_occupancy +=
				self.stream.read(&mut self.buf[LEN_BYTES..buf_end])?;

			if self.buf_occupancy == buf_end {
				// read message to completion!
				let decoded: M = bincode::deserialize(
					&self.buf[LEN_BYTES..buf_end]
				).expect("jirre");
				self.buf_occupancy = 0;
				self.payload_bytes = None;
				return Ok(decoded);
			}
		}
		Err(TryRecvError::ReadNotReady)
	}
}

impl Courier for OneThread {
	fn send<M>(&mut self, m: &M) -> Result<(), io::Error>
	where
		M: Message,
	{
		self.trivial_poll();
		let encoded: Vec<u8> = bincode::serialize(&m).expect("nawww");
		println!("about to send {:?}", &encoded);
		let len = encoded.len();
		if len > ::std::u32::MAX as usize {
			panic!("`send()` can only handle payloads up to std::u32::MAX");
		}
		let mut encoded_len = vec![];
		encoded_len.write_u32::<LittleEndian>(len as u32)?;
		// let sloopy = time::Duration::from_millis(100);
		self.stream.write(&encoded_len)?;
		loop {
			// well now I HAVE to write :/
			match self.stream.write(&encoded) {
				Err(ref e) if e.kind() == ErrorKind::WouldBlock => print!("b"),
				Ok(o) => break,
				Err(e) => return Err(e),
			} 
		}
		println!("send done?");
		Ok(())
	}

	fn send_all<'m, I, M>(&'m mut self, m_iter: I) -> (usize, Result<(), io::Error>)
	where 
		M: Message + 'm,
		I: Iterator<Item = &'m M>,
	{
		let mut tot_sent = 0;
	    for m in m_iter {
	    	match self.send(m) {
	    		Ok(_) => tot_sent += 1,
	    		Err(ref e) if e.kind() == ErrorKind::WouldBlock => unreachable!(),
	    		Err(e) => return (tot_sent, Err(e)),
	    	}
	    }
	    (tot_sent, Ok(()))
	}

	fn recv<M>(&mut self) -> Result<M, io::Error>
	where
		M: Message,
	{
		self.trivial_poll();
		if let Ok(msg) = self.inner_try_recv() {
			return Ok(msg);
		}
		let e = &mut self.events as *mut Events;
		unsafe {
			let e = &mut (*e); // I know that inner_try_recv() doesn't use self.events
			loop {	
				self.poll.poll(e, None).expect("poll failed!");
				for event in e.iter() {
					println!("recv spin!");
					match self.inner_try_recv() {
						Err(TryRecvError::ReadNotReady) => (), //spurious
						Ok(msg) => return Ok(msg),
						Err(TryRecvError::Io(e)) => return Err(e),
					}
				}
			}
		}
	}

	fn try_recv<M>(&mut self) -> Result<M, TryRecvError>
	where
		M: Message,
	{
		self.trivial_poll();
		self.inner_try_recv()
	}
}




//////////////////// MIOTEST/////////////////


const S_IN: Token = Token(0);
const C_IN: Token = Token(1);
fn miotest() {
	let addr = "127.0.0.1:13265".parse().unwrap();
	let server = TcpListener::bind(&addr).unwrap();
	let poll = Poll::new().unwrap();

	// Start listening for incoming connections
	poll.register(&server, S_IN, Ready::readable(),
	              PollOpt::edge()).unwrap();

	// Setup the client socket
	let mut sock = TcpStream::connect(&addr).unwrap();

	// Register the socket
	poll.register(&sock, C_IN, Ready::readable() | Ready::writable(),
	              PollOpt::edge()).unwrap();

	// Create storage for events
	let mut events = Events::with_capacity(32);
	let mut cnt = 0;
	let mut buf = [0u8; 256];
	let mut started = false;
	let halt = time::Duration::from_millis(2000);
	loop {
	    poll.poll(&mut events, None).unwrap();
	    for event in events.iter() {
	        match event.token() {
	            S_IN => {
	            	if event.readiness().is_readable() {
	            		// Accept and drop the socket immediately, this will close
		                // the socket and notify the client of the EOF.
		                println!(" saccepting");
		                if let Ok((client_sock, _addr)) = server.accept() {
		                	println!("s accepted");
		                	thread::Builder::new()
		                	.name("handler".to_string())
		                	.spawn(move || {
		                		// client_sock.set_nodelay(true);
		                		server_handle(client_sock);
		                	}).is_ok();
		                }
	            	}
	            },
	            C_IN => {
	            	if event.readiness().is_readable() {
	            		// The server just shuts down the socket, let's just exit
		                // from our event loop.
		                if cnt < 3 {
		                	println!("c readable!");
					        match sock.read(&mut buf) {
					        	Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
					        		println!("c spurious");
					        	},
				    			Ok(bytes) => {
					        		println!("c read {:?} bytes", bytes);
					        		sock.write(&buf[0..bytes]).is_ok();
				    			},
    							Err(e) => {
				    				// println!("c Sock dead?? {:?}", e);
	        			// 			thread::sleep(halt);
				    				// return;
				    			},
				    		}
		                	cnt += 1;
		                } else {
		                	println!("exiting");
		                	drop(sock);
		                	thread::sleep(halt);
		                	return;
		                }
		                println!("c COUNT = {}", cnt);
	            	} else if event.readiness().is_writable() && !started {
	            		println!("c writable");
	            		buf[0] = 4;
	            		buf[1] = 2;
	            		println!("c wrote `42`");
	            		started = true;
	            		sock.write(&buf[0..2]).is_ok();
	            	}
	            },
	            _ => unreachable!(),
	        }
	    }
	}
}

type ThreadHandle = std::thread::JoinHandle<std::net::SocketAddr>;
fn mio_echoserver() -> (ThreadHandle, SocketAddr) {
	for port in 8000..12000 {
		let addr = SocketAddr::new(
			IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
			port,
		);
		if let Ok(listener) = TcpListener::bind(&addr) {
			let h = thread::Builder::new()
			.name(format!("echo_server_listener"))
			.spawn(move || {
				println!("Listener started at addr {:?}", &addr);
				let poll = Poll::new().unwrap();
				poll.register(&listener, Token(0), Ready::readable(),
					PollOpt::edge()).unwrap();
				let mut events = Events::with_capacity(32);
				loop {
				    poll.poll(&mut events, None).unwrap();
				    println!("mmmk");
				    for event in events.iter() {
				    	println!("echo got event! {:?}", &event);
				    	match listener.accept() {
				    		Err(ref e) if e.kind() == ErrorKind::WouldBlock => println!("spurious"), //spurious wakeup
				    		Ok((client_stream, peer_addr)) => {
				    			thread::Builder::new()
					        	.name(format!("handler_for_client@{:?}", peer_addr))
					        	.spawn(move || {
									println!("Client handler thread away!");
					        		client_stream.set_nodelay(true).is_ok();
					        		server_handle(client_stream);
					        	}).is_ok();
				    		},
				    		Err(_) => {
				    			println!("socket dead");
				    			return addr;
				    		}, //socket has died
				    	}
				    }
				}
			});
			if let Ok(h) = h {
				return (h, addr);
			}			
		}
	}
	panic!("Ran out of ports!");
}

fn server_handle(mut stream: TcpStream) {
	let poll = Poll::new().unwrap();
	poll.register(&stream, Token(21), Ready::readable() | Ready::writable(),
	              PollOpt::edge()).unwrap();
	let mut events = Events::with_capacity(64);
	let mut buf = [0u8; 256];
	let halt = time::Duration::from_millis(500);
	println!("s handle started");
	loop {
	    poll.poll(&mut events, None).unwrap();
	     for event in events.iter() {
	     	if !event.readiness().is_readable() {
	     		continue;
	     	}
	        match stream.read(&mut buf) {
	        	Err(ref e) if e.kind() == ErrorKind::WouldBlock => (),
    			Ok(bytes) => {
	        		println!("s read {:?} bytes", bytes);
	        		//thread::sleep(halt);
	        		println!("serv send {:?}", &buf[0..bytes]);
	        		stream.write(&buf[0..bytes]).expect("did fine");
	        		println!("Ok");
	        		println!("writable? {:?}", event.readiness().is_writable());

	        		println!("s wrote {:?} bytes", bytes);
    			},
    			Err(e) => {
    				println!("s Sock dead! {:?}", e);
    				return;
    			},
    		}
	    }
	}
}