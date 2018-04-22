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

At the _lower_ layer, the Middleman interacts at byte-granularity with the `mio::TcpStream` it wraps. Read and write operations must be nested inside the mio event loop to make good use of the `mio` polling mechanism. As such, this is the _real_ connection between the Middleman and the outside world.

At the _upper_ layer, the Middleman allows the user to _send and receive_ messages (structs). These operations have nothing to do with `mio`, and instead perform serialization/deserialization and change the state of the Middleman. Neither of these operations block.

### Example

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



## The special case of `recv_blocking`
`mio` is asynchronous and non-blocking by nature. While this is enough to get any job done, sometimes a blocking recv call is a more ergonomic fit (in cases where exactly one message is eventually expected, for example). However, as the mio polling system may actually _do work_ lazily, this blocking recv requires an alteration in control flow. 
