use super::*;

use mio::{
    *,
    event::Evented,
};

// use ringtail::ByteBuffer;

use ::std::{
	// net,
    io,
    io::{
        Read,
        Write,
        ErrorKind,
    },
    time,
};

// use byteorder::{
//     LittleEndian,
//     ReadBytesExt,
//     WriteBytesExt,
// };


// TODO remove byteorder dependency


#[derive(Debug)]
pub struct Middleman2 {
    stream: mio::net::TcpStream,
    buf: Vec<u8>,
    buf_occupancy: usize,
    payload_bytes: Option<u32>,
}

impl Middleman2 {
    const LEN_BYTES: usize = 4;

    fn check_payload(&mut self) {
        if self.payload_bytes.is_none() && self.buf_occupancy >= 4 {
        	self.payload_bytes = Some(
        		bincode::deserialize(&self.buf[..Self::LEN_BYTES])
        		.unwrap()
        	)
        }
    }

    pub fn new(stream: std::net::TcpStream) -> Middleman2 {
        Self {
            stream: mio::net::TcpStream::from_stream(stream).expect("failed to mio-ify"),
            buf: Vec::with_capacity(128),
            buf_occupancy: 0,
            payload_bytes: None,
        }
    }

    //TODO make this read only ONE message in so that poll::level can see changes ??? actually is that even needed?
    fn read_in(&mut self) -> Result<usize, io::Error> {
        let mut total = 0;
        loop {
            let limit = (self.buf_occupancy + 64) + (self.buf_occupancy);
            if self.buf.len() < limit {
                self.buf.resize(limit, 0u8);
            }
            match self.stream.read(&mut self.buf[self.buf_occupancy..]) {
                Ok(bytes) => {
                    self.buf_occupancy += bytes;
                    total += bytes;
                },
                Err(ref e) if e.kind() == ErrorKind::WouldBlock => {
                    return Ok(total);
                },
                Err(e) => return Err(e),
            };
        }
    }

    pub fn send<M: Message>(&mut self, m: &M) -> Result<(), SendError> {
    	let m_len = bincode::serialized_size(&m)?;
        if m_len > ::std::u32::MAX as u64 {
            return Err(SendError::TooBigToRepresent);
        }
        bincode::serialize_into(&mut self.stream, &(m_len as u32))?;
        bincode::serialize_into(&mut self.stream, m)?;
        Ok(())
    }

    pub fn send_packed(&mut self, msg: & PackedMessage) -> Result<(), io::Error> {
    	let mut sent = 0;
    	let msg_len = msg.0.len();
    	while sent < msg_len {
    		match self.stream.write(& msg.0[sent..msg_len]) {
    			Ok(sent_now) => sent += sent_now,
    			Err(e) => return Err(e),
    		}
    	}
    	Ok(())
    }

    // fn send_bytes(&mut self, bytes: &[u8]) -> Result<(), SendError> {
    // 	assert!(bytes.len() <= ::std::u32::MAX as usize);
    // 	bincode::serialize_into(self.stream, &(bytes.len() as u32))?;
    // 	Ok(())
    // }

    pub fn send_all<'m, I, M>(&'m mut self, msg_iter: I) -> (usize, Result<(), SendError>) 
    where 
        M: Message + 'm,
        I: Iterator<Item = &'m M>,
    {
        let mut total = 0;
        for msg in msg_iter {
            match self.send(msg) {
                Ok(_) => total += 1,
                Err(e) => return (total, Err(e)),
            }
        }
        (total, Ok(()))
    }

    // pub fn discard(&mut self) -> bool {
    // 	self.read_in();
    //     self.check_payload();
    //     if let Some(pb) = self.payload_bytes {
    //         let buf_end = pb as usize + 4;
    //         if self.buf_occupancy >= buf_end {
    //             self.payload_bytes = None;
    //             self.buf.drain(0..buf_end);
    //             self.buf_occupancy -= buf_end;
    //             return true;
    //         }
    //     }
    //     false
    // }

    pub fn recv<M: Message>(&mut self) -> Result<Option<M>, RecvError> {
    	self.read_in(); //TODO

    	//TODO read in only enough for one message

        self.check_payload();
        if let Some(pb) = self.payload_bytes {
            let buf_end = pb as usize + 4;
            if self.buf_occupancy >= buf_end {
                let decoded: M = bincode::deserialize(
                    &self.buf[Self::LEN_BYTES..buf_end]
                )?;
                self.payload_bytes = None;
                self.buf.drain(0..buf_end);
                self.buf_occupancy -= buf_end;
                return Ok(Some(decoded))
            }
        }
        Ok(None)
    }

    pub fn recv_all_into<M: Message>(&mut self, dest_vector: &mut Vec<M>) -> (usize, Result<(), RecvError>) {
    	self.read_in();
        let mut total = 0;
        loop {
            match self.recv::<M>() {
                Ok(None)         => return (total, Ok(())),
                Ok(Some(msg))     => { dest_vector.push(msg); total += 1; },
                Err(e)            => return (total, Err(e)),
            };
        }
    }

    pub fn recv_blocking<M: Message>(&mut self,
                                 poll: &Poll,
                                 events: &mut Events,
                                 my_tok: Token,
                                 extra_events: &mut Vec<Event>,
                                 mut timeout: Option<time::Duration>) -> Result<Option<M>, RecvError> {

        if let Some(msg) = self.recv::<M>()? {
            // trivial case.
            // message was already sitting in the buffer.
            return Ok(Some(msg));
        }
        let started_at = time::Instant::now();
        let mut res = None;
        loop {
            for event in events.iter() {
                let tok = event.token();
                if res.is_none() && tok == my_tok {
                	if ! event.readiness().is_readable() {
                		continue;
                	}
                    // event is relevant!
                    self.read_in()?;
                    match self.recv::<M>() {
                        Ok(Some(msg)) => {
                            // got a message!
                            res = Some(msg);
                        },
                        Ok(None) => (),
                        Err(e) => return Err(e),
                    }    
                } else {
                    extra_events.push(event);
                }
            }
            if let Some(msg) = res {
                // message ready to go. Exiting loop
                return Ok(Some(msg));
            } else {
                poll.poll(events, timeout).expect("poll() failed inside `recv_blocking()`");
                if let Some(t) = timeout {
                    // update remaining timeout
                    let since = started_at.elapsed();
                    if since >= t {
                        // ran out of time
                        return Ok(None); 
                    }
                    timeout = Some(t-since);
                }
            }
        }
    }

    pub fn recv_all_map<F,M>(&mut self, mut func: F) -> (usize, Result<(), RecvError>)
    where M: Message, F: FnMut(&mut Self, M) + Sized {
    	// self.read_in();
        let mut total = 0;
        loop {
            match self.recv::<M>() {
                Ok(None)         => return (total, Ok(())),
                Ok(Some(msg))     => { total += 1; func(self, msg) },
                Err(e)            => return (total, Err(e)),
            };
        }
    }

    pub fn pack_message<M: Message>(m: & M) -> Result<PackedMessage, SendError> {
    	let m_len: usize = bincode::serialized_size(&m)? as usize;
    	if m_len > ::std::u32::MAX as usize {
            return Err(SendError::TooBigToRepresent);
        }
        let tot_len = m_len+4;
    	let mut vec = Vec::with_capacity(tot_len);
    	vec.resize(tot_len, 0u8);
    	bincode::serialize_into(&mut vec[0..4], &(m_len as u32))?;
        bincode::serialize_into(&mut vec[4..tot_len], m)?;
        Ok(PackedMessage(vec))
    }

    pub fn recv_packed(&mut self) -> Result<Option<PackedMessage>, RecvError> {
    	self.read_in();
        self.check_payload();
        if let Some(pb) = self.payload_bytes {
            let buf_end = pb as usize + 4;
            if self.buf_occupancy >= buf_end {
            	let mut vec = self.buf.drain(0..buf_end)
            	.collect::<Vec<_>>();

                self.payload_bytes = None;
                self.buf_occupancy -= buf_end;
                return Ok(Some(PackedMessage(vec)))
            }
        }
        Ok(None)
    }

    // pub fn recv_is_ready(&mut self) -> bool {
    // 	self.read_in();
    //     self.check_payload();
    //     self.payload_bytes.is_some()
    // }

    // pub fn recv_bytes(&mut self, dest_buffer: &mut Vec<u8>) -> Option<u32> {
    // 	// self.read_in();
    //     self.check_payload();
    //     if let Some(pb) = self.payload_bytes {
    //         let buf_end = pb as usize + 4;
    //         if self.buf_occupancy >= buf_end {
    //             dest_buffer.extend_from_slice(
    //                 &self.buf[Self::LEN_BYTES..buf_end]
    //             );
    //             self.payload_bytes = None;
    //             self.buf.drain(0..buf_end);
    //             self.buf_occupancy -= buf_end;
    //             return Some(pb);
    //         }
    //     }
    //     None
    // }

    // pub fn peek<M: Message>(&mut self) -> Result<Option<M>, RecvError> {
    // 	self.read_in();
    //     self.check_payload();
    //     if let Some(pb) = self.payload_bytes {
    //         let buf_end = pb as usize + 4;
    //         if self.buf_occupancy >= buf_end {
    //             let decoded: M = bincode::deserialize(
    //                 &self.buf[Self::LEN_BYTES..buf_end]
    //             )?;
    //             return Ok(Some(decoded))
    //         }
    //     }
    //     Ok(None)
    // }

}


// collection operations

//TODO

// impl Iterator<item=Middleman2> {
// 	fn send_to_all::<M: Message>(self, m: &M) -> Result<(), SendError> {
// 		let m_len = bincode::serialized_size(&m)?;
// 		if m_len > ::std::u32::MAX as usize {
//             return Err(SendError::TooBigToRepresent);
//         }
//         bincode::serialize_into(self.stream, m_len)?;
// 		for mm in self {

// 		}
// 	}
// }

pub struct PackedMessage(Vec<u8>);


impl Evented for Middleman2     {
    fn register(&self, poll: &Poll, token: Token, interest: Ready, opts: PollOpt)
        -> io::Result<()> {
        self.stream.register(poll, token, interest, opts)
    }

    fn reregister(&self, poll: &Poll, token: Token, interest: Ready, opts: PollOpt)
        -> io::Result<()> {
        self.stream.reregister(poll, token, interest, opts)
    }

    fn deregister(&self, poll: &Poll) -> io::Result<()> {
        self.stream.deregister(poll)
    }
}