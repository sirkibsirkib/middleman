////////////////////// TEST SETUP //////////////////

use super::*;
use mio::*;


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
	collections::{
		HashMap,
	},
	io::{
		Read,
		Write,
		ErrorKind,
	},
	thread,
};

//set to true and run tests with `-- --nocapture` for full printing
const DEBUG_PRINTING: bool = false;

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
impl Message for TestMsg {}

const TOK_A: Token = Token(0);
const TOK_B: Token = Token(0);


#[test]
fn single_client_asynch() {
    let (_handle, addr) = mio_echoserver();
    let mut mm = client_middleman(&addr);

    // Register our middleman with the mio::Poll object.
	let poll = Poll::new().unwrap();
	let mut events = Events::with_capacity(128);
	poll.register(&mm, TOK_A, Ready::readable() | Ready::writable(), PollOpt::edge())
		.expect("failed to register!");

	// send two messages to echo server to get things going
    let messages = vec![
    	TestMsg(2, "A".to_owned()), // `A2`
    	TestMsg(3, "B".to_owned()), // `B3`
    ];
    let (total, res) = mm.send_all(messages.iter());
    assert_eq!(total, 2);
    assert!(res.is_ok());

	let mut messages_left = 7;  // {A2, A1, A0,     B3, B2, B1, B0}

	// start the main loop. Typical mio polling pattern
	loop {

		// poll the underlying socket to cause it to progress.
		// unblocks when there is a change of state for any registered mio::Evented object.
		poll.poll(&mut events, None).ok();
		for event in events.iter() {
			mm.read_write(&event).ok();
		}

		// try to recv ready messages
		while let Some(TestMsg(num, string)) = mm.try_recv::<TestMsg>()
		.expect("socket died!") {
			dprintln!("got msg TestMsg({:?}, {:?})", &num, &string);
			messages_left -= 1;
			if num > 0 {
				mm.send(& TestMsg(num-1, string)).ok();
			}
		}
		if messages_left == 0 { break; }
	}
	dprintln!("got all messages!");
}


#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct StressMsg(u32);
impl Message for StressMsg {}


#[test]
fn stress() {
    let (_handle, addr) = mio_echoserver();
    let mut mm = client_middleman(&addr);
	let poll = Poll::new().unwrap();
	let mut events = Events::with_capacity(128);

    poll.register(& mm, TOK_A, Ready::readable() | Ready::writable(), PollOpt::edge())
		.expect("failed to register!");

	let mut sum = 0;
	mm.send(& StressMsg(100)).ok();

	'outer: loop {
		poll.poll(&mut events, None).ok();
		for event in events.iter() {
			mm.read_write(&event).ok();
		}
		while let Some(StressMsg(value)) = mm.try_recv::<StressMsg>().unwrap() {
			if value < 2 {
				sum += value;
				if sum == 100 {
					break 'outer;
				}
			} else {
				let a = value / 2;
				let b = value - a;
				mm.send(& StressMsg(a)).ok();
				mm.send(& StressMsg(b)).ok();
			}
		}
	}
}


#[test]
fn two_clients_asynch() {
	// start the echo server for the test
    let (_handle, addr) = mio_echoserver();

    // create the mio primitives we need
	let poll = Poll::new().unwrap();
	let mut events = Events::with_capacity(128);

    // now with two clients there are two states to keep track of.
    // we use the mio::Token to identify them.
    // we keep track of how many messages each client is still expecting to receive
    let mut states: HashMap<Token, (Middleman, u32)>
    	= HashMap::new();

    for token in vec![TOK_A, TOK_B] {
    	let mut mm = client_middleman(&addr);

    	// Register our middlemen with the mio::Poll object.
	    poll.register(&mm, token, Ready::readable() | Ready::writable(), PollOpt::edge())
			.expect("failed to register!");

		// send some starting messages to get things going
		mm.send(& TestMsg(2, "A".to_owned()) )
			.is_ok();
		mm.send(& TestMsg(3, "B".to_owned()) )
			.is_ok();

		let state = (mm, 7);
		states.insert(token, state);
    };

    // a variable to remember which Middlemen are finished.
	let mut finished: Vec<Token> = vec![];

	// start the main loop. Typical mio polling pattern
	while !states.is_empty() {

		// poll the underlying socket to cause it to progress.
		// unblocks when there is a change of state for any registered mio::Evented object.
		poll.poll(&mut events, None).ok();
		for event in events.iter() {

			// this event is associated with only one middleman.
			let tok = event.token();
			let &mut (ref mut mm, _to_go) = states.get_mut(& tok)
				.expect("unexpected token");
			mm.read_write(&event).ok();
		}

		// now that socket IO is taken care of, we can do the interesting work
		for (tok, &mut (ref mut mm, ref mut to_go)) in states.iter_mut() {
			while let Some(TestMsg(num, string)) = mm.try_recv::<TestMsg>()
			.expect("socket died!") {
				dprintln!("{:?} got msg TestMsg({:?}, {:?})", tok, &num, &string);
				*to_go -= 1;
				if num > 0 {
					mm.send(& TestMsg(num-1, string)).ok();
				}
			}
			if *to_go == 0 {
				// our work here is done! we push our token so we can
				// be removed from the HashMap and dropped.
				// we can
				finished.push(*tok);
			}
		}

		// remove and drop any middlemen that are done with their work
		for f in finished.drain(..) {
			//drop this middleman
			states.remove(& f);
		}
	}
	dprintln!("all Middlemen did their work to completion!");
}


#[test]
fn blocking() {
	// connect to echo server
    let (_handle, addr) = mio_echoserver();
    let mut mm = client_middleman(&addr);

    // Register our middleman with the mio::Poll object.
	let poll = Poll::new().unwrap();
	let mut events = Events::with_capacity(128);
	poll.register(&mm, TOK_A, Ready::readable() | Ready::writable(), PollOpt::edge())
		.expect("failed to register!");

	// send two messages to echo server to get things going
	// we are going to count from 0 to 20
    
    mm.send(& TestMsg(0, String::new()))
    	.expect("send fail");

    // storage for spilled-over events from recv_blocking()
	let mut spillover: Vec<Event> = vec![];
	let bogus_msg = TestMsg(0, "BOGUS".to_owned());

	// start the main loop. Typical mio polling pattern
	'outer: loop {
		// poll the underlying socket to cause it to progress.
		// unblocks when there is a change of state for any registered mio::Evented object.
		poll.poll(&mut events, None).ok();
		for event in events.iter().chain(spillover.drain(..)) {
			mm.read_write(&event).ok();
		}

		// try to recv ready messages
		while let Some(TestMsg(num, _)) = mm.try_recv::<TestMsg>()
		.expect("socket died!") {
			if num == 20 {
				break 'outer;
			}
			for _ in 0..3 {
				mm.send(& bogus_msg).expect("bogus send fail 3");
			}
			mm.send(& TestMsg(num+1, String::new())).expect("real send fail");

			// hijack the control flow until a specific message is received.
			// all unrelated / extra events will spill over into `spillover`
			for _ in 0..3 {
				let got = mm.recv_blocking::<TestMsg>(
					&poll,
					&mut events,
					TOK_A,
					&mut spillover,
					None,
				);
			dprintln!("expecting bogus message: {:?}", &got);
			assert_eq!(&got.expect("err").expect("none"), &bogus_msg);
			}
		}
	}
}

#[test]
fn discard() {
    let (_handle, addr) = mio_echoserver();
    let mut mm = client_middleman(&addr);

	let poll = Poll::new().unwrap();
	poll.register(&mm, TOK_A, Ready::readable() | Ready::writable(), PollOpt::edge())
		.expect("failed to register!");
	let mut events = Events::with_capacity(128);

	let messages: Vec<_> = (0..30).map(|x| TestMsg(x, String::new())).collect();
	mm.send_all(messages.iter()).1.ok();
	let mut next_index = 0;

	while next_index < 30 {
		poll.poll(&mut events, None).ok();
		for event in events.iter() {
			mm.read_write(&event).ok();
		}

		while next_index < 30 {
			if next_index%2 == 0 {
				if let Some(msg) = mm.try_recv::<TestMsg>().unwrap() {
					dprintln!("GOT {:?}", msg);
					assert_eq!(messages[next_index], msg);
					next_index += 1;
				} else {
					break;
				}
			} else {
				//discarding every 2nd message without comparing.
				//Doesn't require knowledge of struct type. 
				if mm.try_discard() {
					dprintln!("DISCARDED");
					next_index += 1;
				} else {
					break;
				}
			}
		}
	}
}

#[test]
fn try_recv_all() {
    let (_handle, addr) = mio_echoserver();
    let mut mm = client_middleman(&addr);

    // Register our middleman with the mio::Poll object.
	let poll = Poll::new().unwrap();
	poll.register(&mm, TOK_A, Ready::readable() | Ready::writable(), PollOpt::edge())
		.expect("failed to register!");
	let mut events = Events::with_capacity(128);

	for i in 0..1000 {
		let msg = TestMsg(i, format!("num={}", i));
		mm.send(& msg).ok();
	}

	// start the main loop. Typical mio polling pattern
	let mut incoming: Vec<TestMsg> = vec![];
	let mut to_go = 100;
	while to_go > 0 {

		// poll the underlying socket to cause it to progress.
		// unblocks when there is a change of state for any registered mio::Evented object.
		poll.poll(&mut events, None).ok();
		for event in events.iter() {
			mm.read_write(&event).ok();
		}

		// try to recv ready messages
		mm.try_recv_all(&mut incoming).1.ok();
		for _msg in incoming.drain(..) {
			to_go -= 1;
		}
	}
	dprintln!("got all messages!");
}


/////////////////// ECHO SERVER FOR TEST CLIENTS ///////////////////


fn client_middleman(addr: &SocketAddr) -> Middleman {
	let stream = TcpStream::connect(&addr)
		.expect("failed to connect!");
	stream.set_nodelay(true).unwrap();
    Middleman::new(stream)
}

type ThreadHandle = std::thread::JoinHandle<std::net::SocketAddr>;

// spawn an echo server on localhost. return the thread handle and the bound ip addr.
fn mio_echoserver() -> (ThreadHandle, SocketAddr) {
	for port in 4000..17000 {
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
	poll.register(&stream, Token(34), Ready::readable() | Ready::writable(),
	              PollOpt::edge()).unwrap();
	let mut events = Events::with_capacity(300);
	let mut buf = [0u8; 2048];
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