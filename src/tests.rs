use super::*;

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
macro_rules! dprintln {
	() => ();
	($fmt:expr) => (if DEBUG_PRINTING {print!(concat!($fmt, "\n"))});
	($fmt:expr, $($arg:tt)*) => (if DEBUG_PRINTING {
		print!(concat!($fmt, "\n"), $($arg)*)
	});
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct TestMsg(pub u32, pub String);

impl Message for TestMsg {}

#[test]
fn echoes() {
	use self::TestMsg;

    let (_handle, addr) = mio_echoserver();

	let stream = TcpStream::connect(&addr).unwrap();
	stream.set_nodelay(true).is_ok();

    let mut middleman = Threadless::new(stream);

    let batch = vec![
		TestMsg(1, "one".to_owned()),
		TestMsg(2, "two".to_owned()),
		TestMsg(3, "three".to_owned()),
		TestMsg(4, "four".to_owned()),
	];
    let last_msg = TestMsg(5, "five".to_owned());

	// send a sequence of messages over the wire
    let (num_sent, result) = middleman.send_all(batch.iter());
    assert_eq!(num_sent, 4);
    result.is_ok();

    // receive the echo'd messages. check they are correct
    for msg in batch.iter() {
    	assert_eq!(
    		// block until we receive one message
    		&middleman.recv::<TestMsg>().unwrap(), 	//message we get
    		msg, 									// message we expect
    	);
    }

	// send a single message over the wire
    middleman.send(&last_msg).is_ok();

    loop { //spin until we receive message 5!

    	// try read a message (nonblocking!)
    	match middleman.try_recv::<TestMsg>() {
    		Err(TryRecvError::ReadNotReady) => {
    			// the message hasn't come back yet!
    		},
    		Ok(m) => {
    			assert_eq!(&m, &last_msg);
    			break;
    		},
    		Err(_) => panic!("Something went wrong!"),
    	}
    }
    //all went well
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
				dprintln!("Listener started at addr {:?}", &addr);
				let poll = Poll::new().unwrap();
				poll.register(&listener, Token(0), Ready::readable(),
					PollOpt::edge()).unwrap();
				let mut events = Events::with_capacity(32);
				loop {
				    poll.poll(&mut events, None).unwrap();
				    for event in events.iter() {
				    	dprintln!("echo got event! {:?}", &event);
				    	match listener.accept() {
				    		Err(ref e) if e.kind() == ErrorKind::WouldBlock => dprintln!("spurious"), //spurious wakeup
				    		Ok((client_stream, peer_addr)) => {
				    			thread::Builder::new()
					        	.name(format!("handler_for_client@{:?}", peer_addr))
					        	.spawn(move || {
									dprintln!("Client handler thread away!");
					        		client_stream.set_nodelay(true).is_ok();
					        		server_handle(client_stream);
					        	}).is_ok();
				    		},
				    		Err(_) => {
				    			dprintln!("socket dead");
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
	dprintln!("s handle started");
	loop {
	    poll.poll(&mut events, None).unwrap();
	     for event in events.iter() {
	     	if !event.readiness().is_readable() {
	     		continue;
	     	}
	        match stream.read(&mut buf) {
	        	Err(ref e) if e.kind() == ErrorKind::WouldBlock => (),
    			Ok(bytes) => {
	        		dprintln!("s read {:?} bytes", bytes);
	        		//thread::sleep(halt);
	        		dprintln!("serv send {:?}", &buf[0..bytes]);
	        		stream.write(&buf[0..bytes]).expect("did fine");
	        		dprintln!("writable? {:?}", event.readiness().is_writable());

	        		dprintln!("s wrote {:?} bytes", bytes);
    			},
    			Err(e) => {
    				dprintln!("s Sock dead! {:?}", e);
    				return;
    			},
    		}
	    }
	}
}
