use super::structs::Middleman;
use super::*;


use mio::*;

use ::std::{
    fmt::Debug,
    time::{
        Duration,
        Instant,
    },
    net::{
        IpAddr,
        Ipv4Addr,
        SocketAddr,
    },
    collections::{
        HashMap,
    },
    thread,
};


#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
enum Msg {
    Hello,
    Greetings(String),
    Number(u32),
}
impl Message for Msg {}


const TOK_A: Token = Token(0);
const TOK_B: Token = Token(1);
const TOK_C: Token = Token(2);


#[test]
#[should_panic]
fn not_ready() {
    let poll = Poll::new().unwrap();

    let addr = start_echo_server();
    let x = mio::net::TcpStream::connect(&addr).unwrap();
    let mut client = Middleman::new(x);

    // no register has occured! the socket isn't ready
    client.send(& Msg::Hello).expect("hope I don't panic! ;)");

    // too late
    poll.register(&client, TOK_A, Ready::readable() | Ready::writable(), PollOpt::edge()).unwrap();
}

#[test]
fn is_ready() {
    let handles = (0..40).map(|_| {
        thread::spawn(move || {
            let poll = Poll::new().unwrap();

            let addr = start_echo_server();
            let x = mio::net::TcpStream::connect(&addr).unwrap();
            let mut client = Middleman::new(x);

            // registered!
            poll.register(&client, TOK_A, Ready::readable(), PollOpt::edge()).unwrap();

            // socket should be ready to send
            client.send(& Msg::Hello).expect("I shouldn't panic");
        })
    }).collect::<Vec<_>>();

    for h in handles {
        h.join().unwrap();
    }
}



#[test]
fn echo_client_std() {
    echo_client(|addr| {
        mio::net::TcpStream::from_stream(
            std::net::TcpStream::connect(&addr).unwrap()
        ).unwrap()
    });
}

#[test]
fn echo_client_mio() {
    echo_client(|addr| {
        mio::net::TcpStream::connect(&addr).unwrap()
    });
}


fn echo_client<F>(connect_method: F)
where
    F: Fn(&SocketAddr) -> mio::net::TcpStream
{
    let mut events = Events::with_capacity(128);
    let poll = Poll::new().unwrap();

    let addr = start_echo_server();
    let mut client = Middleman::new(connect_method(&addr));
    poll.register(&client, TOK_A, Ready::readable(), PollOpt::edge()).unwrap();

    // socket should be ready to send
    client.send(& Msg::Hello).unwrap();
    let poll_sleep = Some(Duration::from_millis(300));

    let mut to_go = 20;

    loop {
        poll.poll(&mut events, poll_sleep).unwrap();
        for _event in events.iter() {
            client.recv_all_map::<_, Msg>(|me, msg| {
                assert_eq!(&msg, &Msg::Hello);
                me.send(& msg).unwrap();
                to_go -= 1;
            }).1.unwrap();

            if to_go <= 0 {
                return;
            }
        }
    }
    //passed the test
}

#[test]
fn safe_echo_client() {
    let mut events = Events::with_capacity(128);
    let poll = Poll::new().unwrap();

    let addr = start_echo_server();
    let x = mio::net::TcpStream::from_stream(
        std::net::TcpStream::connect(&addr).unwrap()
    ).unwrap();
    let mut client = Middleman::new(x);
    poll.register(&client, TOK_A, Ready::writable(), PollOpt::oneshot()).unwrap();

    //DON'T SEND HERE

    let mut to_go = 20;

    while to_go > 0 {
        poll.poll(&mut events, None).unwrap();
        for event in events.iter() {
            match event.token() {
                TOK_A => {
                    client.send(& Msg::Hello).unwrap();
                    poll.reregister(&client, TOK_B, Ready::readable(),
                                  PollOpt::edge()).unwrap();
                },
                TOK_B => {
                    // first message is the previously-sent hello
                    if let Ok(Some(Msg::Hello)) = client.recv::<Msg>() {
                        client.send(& Msg::Hello).unwrap();
                        to_go -= 1;
                    } else { panic!() }

                    // there is no 2nd message
                    assert_eq!( None, client.recv::<Msg>().unwrap());
                },
                _ => unreachable!(),
            }
        }
    }
}

#[test]
fn count_together() {
    let (mut a, mut b) = connected_pair();
    let poll = Poll::new().unwrap();
    let mut events = Events::with_capacity(128);
    poll.register(&a, TOK_A, Ready::readable(), PollOpt::edge()).unwrap();
    poll.register(&b, TOK_B, Ready::readable(), PollOpt::edge()).unwrap();
    a.send(& Msg::Number(0)).unwrap();

    let mut cont = true;

    

    while cont {
        let mut work = |mm: &mut Middleman, msg| {
            if let Msg::Number(n) = msg {
                if n == 50 {
                    cont = false;
                    return;
                }
                mm.send(& Msg::Number(n + 1)).unwrap();
            } else {
                panic!()
            }
        };

        poll.poll(&mut events, None).unwrap();

        for event in events.iter() {
            match event.token() {
                TOK_A => { a.recv_all_map(&mut work); },
                TOK_B => { b.recv_all_map(&mut work); },
                _ => unreachable!(),
            }
        }
    }
}

// #[test]
// fn l
//     let mut events = Events::with_capacity(128);
//     let poll = Poll::new().unwrap();

//     let addr = start_echo_server();
// }


#[test]
fn recv_blocking() {
    let (mut a, b) = connected_pair();
    echo_forever_threaded::<Msg>(b);

    let poll = Poll::new().unwrap();
    let mut events = Events::with_capacity(128);
    poll.register(&a, TOK_A, Ready::readable(), PollOpt::edge()).unwrap();

    a.send(& Msg::Hello).unwrap();

    let bogus = Msg::Greetings(format!("What shall we do with the drunken sailor?"));

    let mut storage = vec![];
    let mut spillover = vec![];

    let mut rounds = 0;
    while rounds < 20 {
        poll.poll(&mut events, None).unwrap();
        for _event in events.iter().chain(spillover.drain(..)) {
            a.recv_all_into::<Msg>(&mut storage).1.unwrap();
        }

        for msg in storage.drain(..) {
            assert_eq!(&msg, &Msg::Hello);

            for _ in 0..3 {
                a.send(& bogus).unwrap();
            }

            for _ in 0..3 {
                let got = a.recv_blocking::<Msg>(&poll, &mut events, TOK_A, &mut spillover, None)
                .unwrap().unwrap();
                assert_eq!(&got, &bogus);
            }

            a.send(& Msg::Hello).unwrap();
            rounds += 1;
        }
    }
}

#[test]
fn send_a_lot() {
    let (mut a, b) = connected_pair();
    echo_forever_threaded::<Msg>(b);

    let poll = Poll::new().unwrap();
    let mut events = Events::with_capacity(128);
    poll.register(&a, TOK_A, Ready::readable(), PollOpt::edge()).unwrap();

    let msg = Msg::Greetings(format!("Lorem Ipsum is simply dummy text of the printing and typesetting industry.\
        Lorem Ipsum has been the industry's standard dummy text ever since the 1500s, when an unknown printer took\
        a galley of type and scrambled it to make a type specimen book. It has survived not only five centuries,\
        but also the leap into electronic typesetting, remaining essentially unchanged. It was popularised in the\
        1960s with the release of Letraset sheets containing Lorem Ipsum passages, and more recently with desktop\
        publishing software like Aldus PageMaker including versions of Lorem Ipsum."));

    let total = 300;
    for _ in 1..total+1 {
        a.send(& msg).unwrap();
    }

    let mut count = 0;
    let mut storage = vec![];
    loop {
        poll.poll(&mut events, None).unwrap();
        for _event in events.iter() {
            a.recv_all_into::<Msg>(&mut storage).1.unwrap();
        }

        for _msg in storage.drain(..) {
            count += 1;
            if count == total {
                return;
            }
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
struct Complex(HashMap<u8,u32>, (u32, String), (i8, bool, [u16;3], ()), ([bool;5], Vec<[u64;2]>));
impl Message for Complex {}

#[test]
fn simple_packed() {
    let (mut a, b) = connected_pair();
    echo_forever_threaded::<Complex>(b);

    let poll = Poll::new().unwrap();
    let mut events = Events::with_capacity(128);
    poll.register(&a, TOK_A, Ready::readable(), PollOpt::edge()).unwrap();

    let struct_form = Complex(
        {let mut x=HashMap::new(); x.insert(5,4); x.insert(21,0); x},
        (5, format!("whatever, really")),
        (-32, false, [5, 3, 9], ()),
        ([true, false, false, false, true], vec![[22,22], [0,4], [8,3]])
    );
    let packed = Middleman::pack_message(& struct_form).unwrap();

    let total = 1;
    for _ in 0..total {
        a.send_packed(& packed).unwrap();
    }

    let mut to_go = total;
    while to_go > 0 {
        poll.poll(&mut events, None).unwrap();
        for _event in events.iter() {
            a.recv_all_map::<_, Complex>(|_mm, msg| {
                assert_eq!(&msg, &struct_form);
                to_go -= 1;
            });
        }
    }
}


#[test]
fn packed_best_case() {
    let (mut a1, b1) = connected_pair();
    let (mut a2, b2) = connected_pair();
    let (mut a3, b3) = connected_pair();
    echo_forever_threaded::<Complex>(b1);
    echo_forever_threaded::<Complex>(b2);
    echo_forever_threaded::<Complex>(b3);

    let poll = Poll::new().unwrap();
    let mut _events = Events::with_capacity(128);

    poll.register(&a1, TOK_A, Ready::readable(), PollOpt::edge()).unwrap();
    poll.register(&a2, TOK_B, Ready::readable(), PollOpt::edge()).unwrap();
    poll.register(&a3, TOK_C, Ready::readable(), PollOpt::edge()).unwrap();

    // some complex message that is nontrivial to serialize
    let struct_form = Complex(
        {let mut x=HashMap::new(); x.insert(5,4); x.insert(21,0); x},
        (5, format!("whatever, really")),
        (-32, false, [5, 3, 9], ()),
        ([true, false, false, false, true], vec![[22,22], [0,4], [8,3]])
    );
    let total = 600;

    let start_1 = Instant::now();
    {// PACKED SENDING MODE
        let packed = Middleman::pack_message(& struct_form).unwrap();
        for _ in 0..total {
            a1.send_packed(& packed).unwrap();
            a2.send_packed(& packed).unwrap();
            a3.send_packed(& packed).unwrap();
        }
    }
    let time_1 = start_1.elapsed();


    let start_2 = Instant::now();
    {// NORMAL SENDING MODE
        for _ in 0..total {
            a1.send(& struct_form).unwrap();
            a2.send(& struct_form).unwrap();
            a3.send(& struct_form).unwrap();
        }
    }
    let time_2 = start_2.elapsed();
    println!("packed: {:?}\nnormal: {:?}\nPacked took {}% of normal",
                time_1, time_2, nanos(time_1) * 100 / nanos(time_2));

    assert!(time_1 <= time_2);
}

fn nanos(dur: Duration) -> u64 {
    dur.as_secs() as u64 * 1_000_000_000
    + dur.subsec_nanos() as u64
} 



////////////////////////////////////////////


fn echo_forever_threaded<M: Message + Debug>(mut mm: Middleman) {
    thread::spawn(move || {
        let mut events = Events::with_capacity(128);
        let poll = Poll::new().unwrap();
        poll.register(&mm, TOK_A, Ready::readable(), PollOpt::edge()).unwrap();

        loop {
            poll.poll(&mut events, None).unwrap();
            for _event in events.iter() {
                loop {
                    match mm.recv::<M>() {
                        Ok(None) => break,
                        Ok(Some(msg)) => { mm.send(& msg).unwrap(); },
                        Err(_) => panic!("echoer crashed!"), 
                    }
                }
            }
        }
    });
}

fn connected_pair() -> (Middleman, Middleman) {
    for port in 200..15000 {
        let addr = SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            port,
        );
        if let Ok(listener) = ::std::net::TcpListener::bind(&addr) {
            let handle = thread::spawn(move || {
                ::std::net::TcpStream::connect(addr).unwrap()
            });
            let a = listener.accept().unwrap().0;
            let b = handle.join().unwrap();

            a.set_nodelay(true).unwrap();
            b.set_nodelay(true).unwrap();
            let (a, b) = (
                mio::tcp::TcpStream::from_stream(a).unwrap(),
                mio::tcp::TcpStream::from_stream(b).unwrap(),
            );
            return (Middleman::new(a), Middleman::new(b))
        }
    }
    panic!("No ports left!")
}

fn avail_token<T>(m: &HashMap<Token, T>) -> Token {
    for token in (1..).map(|x| Token(x)) {
        if !m.contains_key(&token) {
            return token
        }
    }
    panic!("no more tokens to give!");
}

fn start_echo_server() -> SocketAddr {
    for port in 200..16000 {
        let addr = SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            port,
        );
        if let Ok(listener) = mio::net::TcpListener::bind(&addr) {
            // println!("Echo server bound to port {:?}", port);
            thread::spawn(move || {
                let poll = Poll::new().unwrap();
                let mut buf = [0u8; 512];
                let mut events = Events::with_capacity(128);
                poll.register(&listener, Token(0), Ready::readable(), PollOpt::edge()).unwrap();
                let mut streams: HashMap<Token, mio::net::TcpStream> = HashMap::new();
                loop {
                    // println!("echo poll start");
                    let _ = poll.poll(&mut events, None);
                    // println!("echo poll end");
                    for event in events.iter() {
                        match event.token() {
                            Token(0) => {
                                // println!("listener event");
                                // add a new client
                                let (stream, _addr) = listener.accept().unwrap();
                                let client_token = avail_token(&streams);
                                poll.register(&stream, client_token, Ready::readable() | Ready::writable(),
                                        PollOpt::edge()).unwrap();
                                streams.insert(client_token, stream);
                                // println!("Registering new client with token {:?}", client_token);
                            },
                            token => {
                                // println!("echo client did a thing");
                                // handle update for an existing client
                                let mut remove = false;
                                {
                                    let mut stream = streams.get_mut(&token).unwrap();
                                    loop {
                                        use std::io::ErrorKind::WouldBlock;
                                        use std::io::{Read, Write};

                                        match stream.read(&mut buf[..]) {
                                            Err(ref e) if e.kind() == WouldBlock => break,
                                            Ok(bytes) => {
                                                // println!("Bouncing {:?} bytes for client with token {:?}",
                                                //             bytes, token);
                                                // println!("bouncing {:?}", &buf[..bytes]);
                                                if let Err(_) = stream.write_all(&buf[..bytes]) {
                                                    remove = true;
                                                    poll.deregister(stream).unwrap();
                                                    break;
                                                }
                                                // println!("bounced!");
                                            },
                                            Err(_) => {
                                                remove = true;
                                                poll.deregister(stream).unwrap();
                                                break;
                                            }
                                        }
                                    }
                                }
                                if remove {
                                    streams.remove(&token);
                                    // println!("Removing client with token {:?}", token);
                                }
                            },
                        }
                    }
                }
            });
            return addr;
        }
    }
    panic!("NO PORTS");
}