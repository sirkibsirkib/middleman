# Middleman
`Middleman` aims to make sending structures over a network easy. It is built on top of `mio`, and is in the same spirit as the library `wire`. 

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

## Function
This library aims to provide a number of structs that implement the `Middleman` trait. At present, the only one available is `Threadless`, which leans on `mio`'s non-blocking reads to support blocking and non-blocking sends and receives without requiring additional threads.

## Using the library
1. Implement `Message` for any types `T` you want to send or receive. (This involves implementing the `serde` library's serialization traits. In most cases the `serde_derive` macros are just fine).
```Rust
#[derive(Serialize, Deserialize)]
enum X {
	A,
	B(u32),
	C(Vec<u8>),
}
impl middleman::Message for X {}
```


1. Acquire your `mio::TcpStream` object(s). See the documentation for `mio` for more information.
1. (optional) alter _socketoptions_ as desired for your socket (eg: NoDelay).
1. Wrap your stream with some `MiddleMan` implmentor.
```Rust
use middleman::{Middleman, Threadless};
let mut mm = Threadless::new(stream);
```


That's it. Messages from your peer can be received using `recv()` (blocking) or `try_recv()` (non-blocking). 
```Rust
use middleman::{TryRecvError};

// blocking receive
let x1 = mm.recv::<X>().unwrap();

// non-blocking receive
loop {
	do_work();
	match mm.try_recv::<X>() {
		Ok(msg) => {
			...
		},
		Err(TryRecvError::ReadNotReady) => (), // no problem
		Err(TryRecvError::Fatal(e)) => {
			// handle fatal error
		},
	}
}
```

## Examples
See `src/tests.rs` for an example of a typical use case.