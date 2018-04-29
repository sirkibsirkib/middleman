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

## Using it yourself
If you want to build some kind of Tcp-based network program, you'll need to do a few things. Many of these are in common with `mio`, but let's start somewhere. For our example, I will consider the case of setting up a single client-server connection for a baseline.

Before we begin, this is at the core of what you _conceptually_ want on either end of a communication link:
* One `Middleman` that exposes non-blocking functions `send(&T)` and `recv() -> Option<T>`, where `T` is the type of the message structure(s) you wish to send over the network. Easy.

Old versions of `middleman` stopped there. This always presented the problem: When should you call `recv`? The naive solution is just to keep calling it all the time. Thanksfully, `mio` exists to help with exactly that. It relies on polling to lazily do work and unblock when something can potentially progress. So here we see how to get this all working together smoothly:

1. Setup your messages
    1. Define which message types you wish to send over the network (called 'T' in the description above).
    1. Make these structures serializable with `serde`. I would suggest relying on the macros in `serde_derive`.
    1. Implement the marker trait `middleman::Message` for your messages.
    All in all it may leave things looking like this: 
    ```rust
    #[derive(Serialize, Deserialize)]
    enum MyMsg {
        SayHello,
        SendGameState(Box<GameState>),
        Coordinate(f32, f32),
        ...
    }
    impl middleman::Message for MyMsg {}
    ```
1. Setup your mio loop
    1. For each participant, somehow acquire a `mio::net::TcpStream` object connected to the relevant peer(s). This is stuff not unique to `middleman`.
    1. Wrap each tcp stream in a `Middleman`. 
    1. Register your middlemen with their respective `Poll` objects (as you would with the `mio::TcpStream` itself).
    1. Inside the mio poll loop, call some variant of `Middleman::recv` at the appropriate time. Your job is to ensure that you _always drain all the waiting messages_. `recv` will never block, so feel free to spuriously try and recv something.
    1. use your middlemen to `send` as necessary.

That's it. The flow isnt' very different from that of the typical Tcp setting. The dance simply involves linking up your TcpStream, Middleman, and `mio::Poll` objects into a nice bundle such that you can treat

## Where Mio ends and Middleman begins
When implementing high level algorithms, one likes to think not of _bytes_ and _packets_, but rather of discrete _messages_. Enums and Structs are more neat mappings to these theoretical constructs than byte sequences are. Middleman aims to hide all the byte-level stuff, but hide nothing more.

Someone familiar with the use of `mio` for using the select-loop-like construct to poll the progress of one or more `Evented` structures will see the use of `middleman` doesn't change much. 

At a high level, your code may look something like this:
```

let poll = ...
... // setup other mio stuff
let mut mm = Middleman::new(tcp_stream);
poll.register(&mm, MIDDLEMAN_TOK, ...).unwrap();

loop {
    poll.poll(&mut events, ... ).unwrap();
    for event in events.iter() {
        match event.token() {
            MIDDLEMAN_TOK => {
                if mm.recv_all_map<_, MyType>(|mm_ref, msg| {
                    // do something with `msg`
                }).1.is_err() {
                    // handle errors
                }
            },
            ...
            _ => unreachable!(),
        }
    }
}

```
There are ways of approaching how to precisely get at the messages, when to deserialize them and what to do next,
but this is the crux of it: When you get a notification from _poll_, you try to read all waiting messages and handle them. That's it. At any point you can send a message the other way using `mm.send::<MyType>(& msg)`. No extra threads are needed. No busy-waiting spinning is required (thanks to `mio::Poll`).

## The special case of `recv_blocking`
`mio` is asynchronous and non-blocking by nature. However, sometimes a blocking receive is a more ergonomic fit, for instance in cases where exactly one message is expected. Functions `recv_blocking` and `recv_blocking_solo` exist as a compact means of hijacking the polling loop flow temporarily until a message is ready. See the documentation for more details and see the tests for some examples. 

## A note on message size
This library concentrates on flexibility. Messages of the same type can be represented with different sizes at runtime (eg: an empty hashmap takes fewer bytes than a full one). At the end of the day, the size of your enums on the network may be what you hope for. However, watch out for some pathelogical cases that are the result of the way Rust stores things in memory.

```rust
#[derive(Serialize, Deserialize)]
enum Large {
    A([u64; 32]),
    B(bool),
}
impl Message for Large {}

fn test() {
    let packed = PackedMessage::new(& Large::B(true)).unwrap();
    println!("packed bytes {:?}", packed.byte_len());
    println!("memory bytes {:?}", ::std::mem::size_of::<Large>());
}
```

will print
```
packed bytes 9
memory bytes 264
```