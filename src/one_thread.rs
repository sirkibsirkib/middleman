use super::*;
use mio::channel;
use mio::channel::{
	Sender,
	Receiver,
};

// const Particle: u8 =0u8;

pub struct OneThread {
	// ack_r: Receiver<Particle>, 
	poll: Poll,
	events: Events,
	send_s: Sender<Box<Message>>, 
	recv_r: Receiver<Box<Message>>,
	handle: u32, //threadhandle
}

impl Middleman {
	const NUM_EVENTS: usize = 64;
	const SEND_S_TOK: Token = Token(3);
	const RECV_R_TOK: Token = Token(4);


	fn wait_for_ack(&mut self)
}

impl Middleman for OneThread {
	fn new(stream: TcpStream) -> Self {
		// let (ack_s, ack_r) = channel::channel();
		let (send_s, send_r) = channel::channel();
		let (recv_s, recv_r) = channel::channel();

		let poll = Poll::new().unwrap();
		let mut events = Events::with_capacity(Self::NUM_EVENTS);


		let tcp_token = Token(0);
		let send_r_tok = Token(1);
		let recv_s_tok = Token(2);
		poll.register(&stream, tcp_token, Ready::writable() | Ready::readable(),
		              PollOpt::edge()).expect("register failed");
		poll.register(&stream, send_r_tok, Ready::readable(),
		              PollOpt::edge()).expect("register failed");
		poll.register(&stream, recv_s_tok, Ready::writable(),
		              PollOpt::edge()).expect("register failed");


		let h = thread::Builder::new()
    	.name("one_thread_middleman".to_owned())
    	.spawn(move || {
    		loop {
				poll.poll(&mut events, None)
				.expect("poll failed!");
				for event in events.iter() {
        			match event.token() {
        				tcp_token => {

        				},
        				send_r_tok => {

        				},
        				recv_s_tok => {
        					if let Ok(msg) = recv_s.try_recv() {
        						
        					}
        				},
        				_ => unreachable!(),
				}
    		}

    	}).unwrap();


    	let poll = Poll::new()::unwrap();
		let mut events = Events::with_capacity(Self::NUM_EVENTS);
		poll.register(&stream, Self::SEND_S_TOK, Ready::writable(),
		              PollOpt::edge()).expect("register failed");
		poll.register(&stream, Self::RECV_R_TOK, Ready::readable(),
		              PollOpt::edge()).expect("register failed");
    	OneThread {
    		// ack_r: ack_r,
    		poll: poll,
    		events: events,
    		send_s: send_s,
    		recv_r: recv_r,
    		handle: h,
    	}
	}
	fn into_inner(self) -> (TcpStream, Vec<u8>) {
		unimplemented!()
	}
	fn try_recv<M: Message>(&mut self) -> Result<M, TryRecvError>{
		unimplemented!()
	}
	fn recv<M: Message>(&mut self) -> Result<M, FatalError>{
		unimplemented!()
	}
	fn send<M: Message>(&mut self, m: &M) -> Result<(), FatalError> {
		unimplemented!()
	}
	fn send_all<'m, I, M>(&'m mut self, m_iter: I) -> (usize, Result<(), FatalError>)
	where 
		M: Message + 'm,
		I: Iterator<Item = &'m M>
	{

		unimplemented!()
	}
}

