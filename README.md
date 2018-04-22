# Middleman
`Middleman` is a library for sending and receiving serializable data structures over a TCP connection, abstracting away from the raw bytes. This project draws inspiration from an older library, `wire`, but is intended to play nicely with the `mio` polling system. 

```Rust
 struct M: Message 
    ↑       ╷
    ┆       ┆

    Middleman (⌐■_■)
       TCP
    ~~~~~~~~~
    ▲       ▼    
    ▲ bytes ▼    
    ▲       ▼
    ~~~~~~~~~    
       TCP
    Middleman (⌐■_■)

    ┆       ┆
    ╵       ↓
 struct M: Message 
```

## (Send+Recv) vs (Read+Write)
The _meat and potatoes_ of this library is the `Middleman` structure. Internally, it is a little state machine that stores incoming and outgoing bytes in small buffers.

At the _lower_ layer, the Middleman interacts at byte-granularity with the `mio::TcpStream` wrapped within. Read and write operations must be nested inside the mio event loop to make good use of the polling mechanism. As such, this layer is the _real_ connection between the Middleman and the outside world.

At the _upper_ layer, the Middleman allows the user to _send and receive_ messages (structs). These operations have nothing to do with `mio`, and instead perform serialization/deserialization and change the state of the Middleman. Neither of these operations block.

### Typical Layout
Below is a skeleton for the expected use case of a `Middleman`. Creation of the `Poll` and `Event` objects (typical mio stuff) is omitted.

```rust
const CLIENT: Token = Token(0);
...
let mut mm = Middleman::new(mio_tcp_stream);
poll.register(&mm, CLIENT, Ready::readable() | Ready::writable(), PollOpt::edge())
	.expect("failed to register!");

let mut incoming: Vec<TestMsg> = vec![];
loop {
	mm.write_out().ok();
	
	poll.poll(&mut events, None).ok();
	for event in events.iter() {
		match event.token() {
			MIO_TOKEN => mm.read_in().ok(),
			_ => unreachable!(),
		}
	}
	
	mm.try_recv_all(&mut incoming).1.ok();
	for _msg in incoming.drain(..) {
		// do stuff here
	}
}
```

Thanks to `poll.poll(...)`, the loop blocks whenever there is nothing new to do, but is triggered the instant something changes with the state of the Middleman.

## The special case of `recv_blocking`
`mio` is asynchronous and non-blocking by nature. However, sometimes a blocking receive is a more ergonomic fit (in cases where exactly one message is eventually expected, for example). However, as the mio polling system may actually _do work_ lazily, this blocking recv requires an alteration in control flow. 

To facilitate this, the function `recv_blocking` requires some extra arguments:

```rust
...
let mut spillover: Vec<Event> = vec![];
loop {
	mm.write_out().ok();
	poll.poll(&mut events, None).ok();
	for _event in events.iter().chain(spillover.drain(..)) { 
	// need to also traverse events that may have been skimmed over during `recv_blocking` call

		// only one token is registered. no need to check
		mm.read_in().ok();
	}
	...
	match mm.recv_blocking::<TestMsg>(
		&poll,
		&mut events,
		CLIENT,
		&mut spillover,
		None, // optional timeout
	) {
		Err(e) 			=> ... ,	
		Ok(None)		=> ... , // timed out
		Ok(Some(msg)) 	=> ... ,
	}
}
```
Note that now, each loop, we need to iterate over both _new_ events and also events that were consumed during a previous call to `recv_blocking`. The function works by temporarily hijacking the control flow of the event loop, and (in this way), buffering messages it encounters until it can use those that signal new bytes to read.