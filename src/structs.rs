use super::*;

use mio::{
    *,
    event::Evented,
};

use ::std::{
    io,
    io::{
        Read,
        Write,
        ErrorKind,
    },
    time,
};


#[derive(Debug)]
pub struct Middleman {
    stream: mio::net::TcpStream,
    buf: Vec<u8>,
    buf_occupancy: usize,
    payload_bytes: Option<u32>,
}

impl Middleman {    fn check_payload(&mut self) {
        if self.payload_bytes.is_none() && self.buf_occupancy >= 4 {
        	self.payload_bytes = Some(
        		bincode::deserialize(&self.buf[..4])
        		.unwrap()
        	)
        }
    }

    /// Create a new Middleman structure to wrap the given Mio TcpStream.
    /// The Middleman implements `mio::Evented`, but delegates its functions to this given stream
    /// As such, registering the Middleman and registering the TcpStream are anaologous.
    pub fn new(stream: mio::net::TcpStream) -> Middleman {
    	Self {
            stream: stream,
            buf: Vec::with_capacity(128),
            buf_occupancy: 0,
            payload_bytes: None,
        }
    }

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

    /// Write the given message directly into the TcpStream. Returns `Err` variant if there
    /// is a problem serializing or writing the message. The call returns Ok(()) once the bytes
    /// are entirely written to the stream.
    pub fn send<M: Message>(&mut self, m: &M) -> Result<(), SendError> {
    	self.send_packed(
    		& PackedMessage::new(m)?
    	)?;
    	Ok(())
    }

    /// See `send`. This variant can be useful to
    /// avoid the overhead of repeatedly packing a message for whatever reason, eg: sending
    /// the same message using multiple Middleman structs. 
    ///
    /// Note that this function does NOT check for internal consistency of the packed message.
    /// So, if this message was constructed by a means other than `Packed::new`, then the
    /// results may be unpredictable.
    pub fn send_packed(&mut self, msg: & PackedMessage) -> Result<(), io::Error> {
    	self.stream.write_all(&msg.0)
    }

    /// Conume an iterator over some Message structs, sending them all in the order traversed (see `send`).
    /// Returns (a,b) where a gives the total number of messages sent successfully and where b is Ok if
    /// nothing goes wrong and an error otherwise. In the event of the first error, no more messages will be sent.
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

    /// See `send_all` and `send_packed`. This uses the message iterator from the former and
    /// the packed messages from the latter. 
    pub fn send_all_packed<'m, I>(&'m mut self, packed_msg_iter: I) -> (usize, Result<(), io::Error>) 
    where 
        I: Iterator<Item = &'m PackedMessage>,
    {
        let mut total = 0;
        for msg in packed_msg_iter {
            match self.send_packed(msg) {
                Ok(_) => total += 1,
                Err(e) => return (total, Err(e)),
            }
        }
        (total, Ok(()))
    }

    /// Attempt to dedserialize some data in the receiving buffer into a single 
    /// complete structure with the given type M. If there is insufficient data
    /// at the moment, Ok(None) is returned. 
    ///
    /// As the type is provided by the reader, it is possible for the sent
    /// message to be misinterpreted as a different type. At best, this is detected
    /// by a failure in deserialization. If an error occurs, the data is not consumed
    /// from the Middleman. Subsequent reads will operate on the same data.
    ///
    /// NOTE: The correctness of this call depends on the sender sending an _internally consistent_
    /// `PackedMessage`. If you (or the sender) are manually manipulating the internal state of 
    /// sent messages this may cause errors for the receiver. If you are sticking to the Middleman API
    /// and treating each `PackedMessage` as a black box, everything should be fine.
    pub fn recv<M: Message>(&mut self) -> Result<Option<M>, RecvError> {
    	self.read_in()?;
        self.check_payload();
        if let Some(pb) = self.payload_bytes {
            let buf_end = pb as usize + 4;
            if self.buf_occupancy >= buf_end {
                let decoded: M = bincode::deserialize(
                    &self.buf[4..buf_end]
                )?;
                self.payload_bytes = None;
                self.buf.drain(0..buf_end);
                self.buf_occupancy -= buf_end;
                return Ok(Some(decoded))
            }
        }
        Ok(None)
    }

    /// See `recv`. Will repeatedly call recv() until the next message is not yet ready. 
    /// Recevied messages are placed into the buffer `dest_vector`. The return result is (a,b)
    /// where a is the total number of message successfully received and where b is OK(()) if all goes well
    /// and some Err otherwise. In the event of the first error, the call will return and not receive any further. 
    pub fn recv_all_into<M: Message>(&mut self, dest_vector: &mut Vec<M>) -> (usize, Result<(), RecvError>) {
        let mut total = 0;
        loop {
            match self.recv::<M>() {
                Ok(None)         => return (total, Ok(())),
                Ok(Some(msg))     => { dest_vector.push(msg); total += 1; },
                Err(e)            => return (total, Err(e)),
            };
        }
    }

    /// Hijack the mio event loop, reading and writing to the socket as polling allows.
    /// Events not related to the recv() of this middleman (determined from the provided mio::Token)
    /// are pushed into the provided extra_events vector. Returns Ok(Some(_)) if a
    /// message was successfully received. May return Ok(None) if the user provides as timeout some 
    // non-none Duration. Returns Err(_) if something goes wrong with reading from the socket or
    /// deserializing the message. See try_recv for more information.

    /// WARNING: The user should take care to iterate over these events also, as without them all the
    /// Evented objects registered with the provided poll object might experience lost wakeups.
    /// It is suggested that in the event of any recv_blocking calls in your loop, you extend the event
    /// loop with a drain() on the same vector passed here as extra_events (using the iterator chain function, for example.)
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



    /// See `recv_blocking`. This function is intended as an alternative for use
    /// for cases where it is _certain_ that this Middleman is the only registered `mio::Evented`
    /// for the provided `Poll` and `Events` objects. Thus, the call _WILL NOT CHECK_ the token at all,
    /// presuming that all events are associated with this middleman.
    pub fn recv_blocking_solo<M: Message>(&mut self,
                                 poll: &Poll,
                                 events: &mut Events,
                                 mut timeout: Option<time::Duration>) -> Result<Option<M>, RecvError> {

        if let Some(msg) = self.recv::<M>()? {
            // trivial case.
            // message was already sitting in the buffer.
            return Ok(Some(msg));
        }
        let started_at = time::Instant::now();
        loop {
            for event in events.iter() {
                if event.readiness().is_readable(){
                    // event is relevant!
                    self.read_in()?;
                    match self.recv::<M>() {
                        Ok(Some(msg)) => return Ok(Some(msg)),
                        Ok(None) => (),
                        Err(e) => return Err(e),
                    }    
                }
            }
            poll.poll(events, timeout).expect("poll() failed inside `recv_blocking_solo()`");
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

    /// Similar to `recv_all_into`, but rather than storing each received message 'm',
    /// the provided function is called with arguments (self, m) where self is `&mut self`.
    /// This allows for ergonomic utility of the received messages using a closure.
    pub fn recv_all_map<F,M>(&mut self, mut func: F) -> (usize, Result<(), RecvError>)
    where M: Message, F: FnMut(&mut Self, M) + Sized {
        let mut total = 0;
        loop {
            match self.recv::<M>() {
                Ok(None)          => return (total, Ok(())),
                Ok(Some(msg))     => { total += 1; func(self, msg) },
                Err(e)            => return (total, Err(e)),
            };
        }
    }

    /// Combination of `recv_all_map` and `recv_packed`.
    pub fn recv_all_packed_map<F>(&mut self, mut func: F) -> (usize, Result<(), RecvError>)
    where F: FnMut(&mut Self, PackedMessage) + Sized {
        let mut total = 0;
        loop {
            match self.recv_packed() {
                Ok(None)          => return (total, Ok(())),
                Ok(Some(packed))  => { total += 1; func(self, packed) },
                Err(e)            => return (total, Err(e)),
            };
        }
    }

    /// Similar to `recv`, except builds (instead of some M: Message), a `PackedMessage` object.
    /// These packed messages can be deserialized later, sent on the line without knowledge of the 
    /// message type etc.
    pub fn recv_packed(&mut self) -> Result<Option<PackedMessage>, RecvError> {
    	self.read_in()?;
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

    /// Similar to `recv_packed`, but the potentially-read bytes are not actually removed
    /// from the stream. The message will _still be there_.
    pub fn peek_packed(&mut self) -> Result<Option<PackedMessage>, RecvError> {
        if let Some(pb) = self.payload_bytes {
            let buf_end = pb as usize + 4;
            if self.buf_occupancy >= buf_end {
                return Ok(
                    Some(
                        PackedMessage::from_raw(
                            self.buf[4..buf_end].to_vec()
                        )
                    )
                )
            }
        }
        Ok(None)
    }
}



/// This structure represents the serialized form of some `Message`-implementing structure.
/// Dealing with a PackedMessage may be suitable when:
/// (1) You need to send/store/receive a message but don't need to actually _use_ it yourself.
/// (2) You want to serialize a message once, and send it multiple times.
/// (3) You want to read and discard a message whose type is unknown.
///
/// NOTE: The packed message maps 1:1 with the bytes that travel over the TcpStream. As such,
/// packed messages also contain the 4-byte length preable. The user is discocuraged from
/// manipulating the contents of a packed message. The `recv` statement relies on consistency
/// of packed messages
pub struct PackedMessage(Vec<u8>);
impl PackedMessage {

    /// Create a new PakcedMessage from the given `Message`-implementing struct
    pub fn new<M: Message>(m: &M) -> Result<Self, PackingError>  {
        let m_len: usize = bincode::serialized_size(&m)? as usize;
        if m_len > ::std::u32::MAX as usize {
            return Err(PackingError::TooBigToRepresent);
        }
        let tot_len = m_len+4;
        let mut vec = Vec::with_capacity(tot_len);
        vec.resize(tot_len, 0u8);
        bincode::serialize_into(&mut vec[0..4], &(m_len as u32))?;
        bincode::serialize_into(&mut vec[4..tot_len], m)?;
        Ok(PackedMessage(vec))
    }

    /// Attempt to unpack this Packedmessage given a type hint. This may fail if the 
    /// PackedMessage isn't internally consistent or the type doesn't match that
    /// of the type used for serialization.
	pub fn unpack<M: Message>(&self) -> Result<M, Box<bincode::ErrorKind>> {
        bincode::deserialize(&self.0[4..])
	}

    /// Unwrap the byte buffer comprising this PackedMessage
    #[inline] pub fn into_raw(self) -> Vec<u8> { self.0 }

    /// Accept the given byte buffer as the basis for a PackedMessage
    ///
    /// WARNING: Use this at your own risk! The `recv` functions and their variants rely on
    /// the correct contents of messages to work correcty.
    ///
    /// NOTE: The first 4 bytes of a the buffer are used to store the length of the payload.
    #[inline] pub fn from_raw(v: Vec<u8>) -> Self { PackedMessage(v) }

    /// Return the number of bytes this packed message contains. Maps 1:1 with
    /// the bit complexity of the message sent over the network.
    #[inline] pub fn byte_len(&self) -> usize { self.0.len() }

    /// Acquire an immutable reference to the internal buffer of the packed message.
    #[inline] pub fn get_raw(&self) -> &Vec<u8> { &self.0 }

    /// Acquire a mutable reference to the internal buffer of the packed message.
    ///
    /// WARNING: Contents of a PackedMessage represent a delicata internal state. Sending an
    /// internally inconsistent PackedMessage will compromise the connection. Use at your own risk!
    #[inline] pub fn get_mut_raw(&mut self) -> &mut Vec<u8> { &mut self.0 }
}



impl Evented for Middleman     {
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