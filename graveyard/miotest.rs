
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
				    				println!("c Sock dead?? {:?}", e);
	        						thread::sleep(halt);
				    				return;
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