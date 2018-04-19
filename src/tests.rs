use super::*;

#[macro_use] extern crate serde_derive;

use ::mio::tcp::{
	TcpListener,
	TcpStream,
};

use ::std::{
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
	thread,
};

const DEBUG_PRINTING: bool = false;
//set to true and run tests with `-- --nocapture` for full printing
macro_rules! dprintln {
	() => ();
	($fmt:expr) => (if DEBUG_PRINTING {print!(concat!($fmt, "\n"))});
	($fmt:expr, $($arg:tt)*) => (if DEBUG_PRINTING {
		print!(concat!($fmt, "\n"), $($arg)*)
	});
}

// Here is the struct we will be sending over the network
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct TestMsg(pub u32, pub String);

// Need to mark it with the `Message` trait
impl Message for TestMsg {}

#[test]
fn echoes() {
	use self::TestMsg;

	// start up a server that accepts at 'addr' and will echo all bytes we send
	// this echo server is just a regular mio implementation (no middleman)
	dprintln!("Starting echo server");
    let (_handle, addr) = mio_echoserver();

    // create out client mio::TcpStream.
	let stream = TcpStream::connect(&addr).unwrap();
	dprintln!("Connected to echo server");
	// Nodelay==true for speed. Works with and without.
	stream.set_nodelay(true).unwrap();

	// Create our middleman object to protect the `TcpStream` object.
	dprintln!("Creating middleman");
    let mut middleman = Threadless::new(stream);

    // create a sequence of messages we want to send to the echo server
    let batch = vec![
		TestMsg(1, "one".to_owned()),
		TestMsg(2, "two".to_owned()),
		TestMsg(3, "three".to_owned()),
		TestMsg(4, "four".to_owned()),
	];
    let last_msg = TestMsg(5, "five".to_owned());

	// send a sequence of messages over the wire
	dprintln!("sending messages 1,2,3,4");
    let (num_sent, result) = middleman.send_all(batch.iter());
    assert_eq!(num_sent, 4);
    result.unwrap();

    // receive the echo'd messages. check they are correct
    for msg in batch.iter() {
		dprintln!("got {:?}", msg);
    	assert_eq!(
    		// block until we receive one message
    		&middleman.recv::<TestMsg>().unwrap(), 	//message we get
    		msg, 									// message we expect
    	);
    }

	// send a single message over the wire
	dprintln!("sending message 5");
    middleman.send(&last_msg).unwrap();

    loop { //spin until we receive message 5!

    	// try read a message (nonblocking!)
    	match middleman.try_recv::<TestMsg>() {
    		Err(TryRecvError::ReadNotReady) => {
    			// the message hasn't come back yet!
				dprintln!("... waiting for last msg ...");
    		},
    		Ok(msg) => {
				dprintln!("got {:?}", msg);
    			assert_eq!(&msg, &last_msg);
				dprintln!("got last message");
    			break;
    		},
    		Err(_) => panic!("Something went wrong!"),
    	}
    }
    //all went well
}

#[test]
fn many_blocking() {
	let num_threads = 20;
	let num_messages = 10;

    let (_handle, addr) = mio_echoserver();
	let mut handles = (0..num_threads)
	.map(|x| {
		thread::spawn(move || {
			let stream = TcpStream::connect(&addr).unwrap();
			stream.set_nodelay(true).unwrap();
		    let mm = Threadless::new(stream);
			blocking(mm, x, &format!("[{:02}]", x), num_messages);
		})
	})
	.collect::<Vec<_>>();
	for h in handles.drain(..) {
		h.join().unwrap();
	}
}


#[test]
fn many_nonblocking() {
	let num_threads = 5;
	let num_messages = 10;

    let (_handle, addr) = mio_echoserver();
	let mut handles = (0..num_threads)
	.map(|x| {
		thread::spawn(move || {
			let stream = TcpStream::connect(&addr).unwrap();
			stream.set_nodelay(true).unwrap();
		    let mm = Threadless::new(stream);
			nonblocking(mm, x, &format!("[{:02}]", x), num_messages)
		})
	})
	.collect::<Vec<_>>();
	for h in handles.drain(..) {
		h.join().unwrap();
	}
}

fn blocking<M: Middleman>(mut mm: M, index: u32, name: &str, num_messages: u32) {
	let messages = (0..num_messages)
	.map(|x| TestMsg(x, name.to_owned()))
	.collect::<Vec<_>>();
	for m in messages.iter() {
		mm.send(m).unwrap();
	}
	for sent in messages.iter() {
		let msg = mm.recv::<TestMsg>()
		.expect("crashed on recv!");
		assert_eq!(&msg, sent);
	}
	dprintln!("t index {} got all messages correctly", index);
}


fn nonblocking<M: Middleman>(mut mm: M, index: u32, name: &str, num_messages: u32) {
	let messages = (0..num_messages)
	.map(|x| TestMsg(x, name.to_owned()))
	.collect::<Vec<_>>();

	for m in messages.iter() {
		mm.send(m).unwrap();
	}

	let mut got = vec![];
	let patience = time::Duration::from_millis(5000);
	let sleepy = time::Duration::from_millis(1);
	let start = time::Instant::now();	

	let mut loops = 0;
	while got.len() < num_messages as usize && start.elapsed() <= patience {
		loops += 1;
		thread::sleep(sleepy);
		match mm.try_recv::<TestMsg>() {
			Ok(TestMsg(a, _b)) => {
				got.push(a);
			},
			Err(TryRecvError::ReadNotReady) => (), // spin!
			Err(TryRecvError::Fatal(f)) => {
				dprintln!("client {} crashed with {:?}", index, f);
				return;
			}
		}
	}
	dprintln!("t index {} did {} loops. got {}/{}\t{:?}", index, loops, got.len(), num_messages, &got);
}


type ThreadHandle = std::thread::JoinHandle<std::net::SocketAddr>;

// spawn an echo server on localhost. return the thread handle and the bound ip addr.
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
				dprintln!("[echo] listener started at addr {:?}", &addr);
				let poll = Poll::new().unwrap();
				poll.register(&listener, Token(0), Ready::readable(),
					PollOpt::edge()).unwrap();
				let mut events = Events::with_capacity(32);
				loop {
				    poll.poll(&mut events, None).unwrap();
				    for _event in events.iter() {
				    	match listener.accept() {
				    		Err(ref e) if e.kind() == ErrorKind::WouldBlock => (), //spurious wakeup
				    		Ok((client_stream, peer_addr)) => {
				    			thread::Builder::new()
					        	.name(format!("handler_for_client@{:?}", peer_addr))
					        	.spawn(move || {
					        		client_stream.set_nodelay(true).unwrap();
					        		server_handle(client_stream);
					        	}).unwrap();
				    		},
				    		Err(_) => {
				    			dprintln!("[echo] listener socket died!");
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
	panic!("[echo] Ran out of ports to try!");
}

fn server_handle(mut stream: TcpStream) {
	let poll = Poll::new().unwrap();
	poll.register(&stream, Token(21), Ready::readable() | Ready::writable(),
	              PollOpt::edge()).unwrap();
	let mut events = Events::with_capacity(128);
	let mut buf = [0u8; 1024];
	loop {
	    poll.poll(&mut events, None).unwrap();
	    for event in events.iter() {
	    	if !event.readiness().is_readable() {
	    		continue;
	    	}
	        match stream.read(&mut buf) {
	        	Err(ref e) if e.kind() == ErrorKind::WouldBlock => (),
    			Ok(bytes) => {
    				if bytes > 0 {
		        		dprintln!("[echo] sent {} bytes.", bytes);
		        		stream.write(&buf[0..bytes]).expect("write failed");
    				}
    			},
    			Err(_e) => {
    				dprintln!("[echo] Client has dropped socket");
    				return;
    			},
    		}
	    }
	}
}

fn hex_string(bytes: &[u8]) -> String {
	let mut s = String::new();
	s.push('[');
	for b in bytes.iter() {
		s.push_str(&format!("{:0X} ", *b));
	}
	s.push(']');
	s
}
