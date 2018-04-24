use super::structs2::Middleman2;
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
    // io::{
    //     // Read,
    //     // Write,
    //     // ErrorKind,
    // },
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
fn count_together() {
    let (mut a, mut b) = connected_pair();
    let poll = Poll::new().unwrap();
    let mut events = Events::with_capacity(128);
    poll.register(&a, TOK_A, Ready::readable(), PollOpt::edge()).unwrap();
    poll.register(&b, TOK_B, Ready::readable(), PollOpt::edge()).unwrap();
    a.send(& Msg::Number(0)).ok();

    let mut cont = true;

    

    while cont {
        let mut work = |mm: &mut Middleman2, msg| {
            if let Msg::Number(n) = msg {
                if n == 50 {
                    cont = false;
                    return;
                }
                mm.send(& Msg::Number(n + 1)).ok();
            } else {
                panic!()
            }
        };

        poll.poll(&mut events, None).ok();

        for event in events.iter() {
            match event.token() {
                TOK_A => { a.recv_all_map(&mut work); },
                TOK_B => { b.recv_all_map(&mut work); },
                _ => unreachable!(),
            }
        }
    }
}



#[test]
fn recv_blocking() {
    let (mut a, b) = connected_pair();
    echo_forever_threaded::<Msg>(b);

    let poll = Poll::new().unwrap();
    let mut events = Events::with_capacity(128);
    poll.register(&a, TOK_A, Ready::readable(), PollOpt::edge()).unwrap();

    a.send(& Msg::Hello).ok();

    let bogus = Msg::Greetings(format!("What shall we do with the drunken sailor?"));

    let mut storage = vec![];
    let mut spillover = vec![];

    let mut rounds = 0;
    while rounds < 20 {
        poll.poll(&mut events, None).ok();
        for event in events.iter().chain(spillover.drain(..)) {
            a.recv_all_into::<Msg>(&mut storage).1.ok();
        }

        for msg in storage.drain(..) {
            assert_eq!(&msg, &Msg::Hello);

            for _ in 0..3 {
                a.send(& bogus).ok();
            }

            for _ in 0..3 {
                let got = a.recv_blocking::<Msg>(&poll, &mut events, TOK_A, &mut spillover, None)
                .unwrap().unwrap();
                assert_eq!(&got, &bogus);
            }

            a.send(& Msg::Hello).ok();
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

    let total = 1000;
    for i in 1..total+1 {
        a.send(& msg).ok();
    }

    let mut count = 0;
    let mut storage = vec![];
    loop {
        poll.poll(&mut events, None).ok();
        for event in events.iter() {
            a.recv_all_into::<Msg>(&mut storage).1.ok();
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
    let packed = Middleman2::pack_message(& struct_form).unwrap();

    let total = 1;
    for _ in 0..total {
        a.send_packed(& packed).ok();
    }

    let mut to_go = total;
    while to_go > 0 {
        poll.poll(&mut events, None).ok();
        for event in events.iter() {
            a.recv_all_map::<_, Complex>(|mm, msg| {
                to_go -= 1;
            });
        }
    }
}


#[test]
fn many_packed() {

    let (mut a1, b1) = connected_pair();
    let (mut a2, b2) = connected_pair();
    let (mut a3, b3) = connected_pair();
    echo_forever_threaded::<Complex>(b1);
    echo_forever_threaded::<Complex>(b2);
    echo_forever_threaded::<Complex>(b3);

    let poll = Poll::new().unwrap();
    let mut events = Events::with_capacity(128);

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
        let packed = Middleman2::pack_message(& struct_form).unwrap();
        for i in 0..total {
            a1.send_packed(& packed).ok();
            a2.send_packed(& packed).ok();
            a3.send_packed(& packed).ok();
        }

        let (mut x, mut y, mut z) = (total, total, total); // "to go" counters
        loop {
            poll.poll(&mut events, None).ok();
            for event in events.iter() {
                match event.token() {
                    TOK_A => a1.recv_all_map::<_, Complex>(|_mm, _msg| {
                        x -= 1;
                    }),
                    TOK_B => a2.recv_all_map::<_, Complex>(|_mm, _msg| {
                        y -= 1;
                    }),
                    TOK_C => a3.recv_all_map::<_, Complex>(|_mm, _msg| {
                        z -= 1;
                    }),
                    _ => unreachable!(),
                };
            }
            if (x,y,z) == (0,0,0) {
                break; //work done
            }
        }
    }
    let time_1 = start_1.elapsed();


    let start_2 = Instant::now();
    {// NORMAL SENDING MODE
        for i in 0..total {
            a1.send(& struct_form).ok();
            a2.send(& struct_form).ok();
            a3.send(& struct_form).ok();
        }

        let (mut x, mut y, mut z) = (total, total, total); // "to go" counters
        loop {
            poll.poll(&mut events, None).ok();
            for event in events.iter() {
                match event.token() {
                    TOK_A => a1.recv_all_map::<_, Complex>(|_mm, _msg| {
                        x -= 1;
                    }),
                    TOK_B => a2.recv_all_map::<_, Complex>(|_mm, _msg| {
                        y -= 1;
                    }),
                    TOK_C => a3.recv_all_map::<_, Complex>(|_mm, _msg| {
                        z -= 1;
                    }),
                    _ => unreachable!(),
                };
            }
            if (x,y,z) == (0,0,0) {
                break; //work done
            }
        }
    }
    let time_2 = start_2.elapsed();
    println!("packed: {:?}\nnormal: {:?}\nPacked took {}% of normal",
                time_1, time_2, nanos(time_1) * 100 / nanos(time_2));
}

fn nanos(dur: Duration) -> u64 {
    dur.as_secs() as u64 * 1_000_000_000
    + dur.subsec_nanos() as u64
} 

//////////////////////////////////////////////


fn echo_forever_threaded<M: Message + Debug>(mut mm: Middleman2) {
    thread::spawn(move || {
        let mut events = Events::with_capacity(128);
        let poll = Poll::new().unwrap();
        poll.register(&mm, TOK_A, Ready::readable(), PollOpt::edge()).unwrap();

        loop {
            poll.poll(&mut events, None).ok();
            for event in events.iter() {
                loop {
                    match mm.recv::<M>() {
                        Ok(None) => break,
                        Ok(Some(msg)) => { mm.send(& msg).ok(); },
                        Err(_) => panic!("echoer crashed!"), 
                    }
                }
            }
        }
    });
}

fn connected_pair() -> (Middleman2, Middleman2) {
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

            a.set_nodelay(true).ok();
            b.set_nodelay(true).ok();

            return (Middleman2::new(a), Middleman2::new(b))
        }
    }
    panic!("No ports left!")
}