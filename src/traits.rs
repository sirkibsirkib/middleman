use super::*;
use serde::{
	Serialize, 
	de::DeserializeOwned,
};
use mio::tcp::TcpStream;

/// Marks the structure as a `Message`. Being a message relies on being serializable so the
/// Middleman can send them on the network.
///
/// # Examples
///
/// ```
/// #[derive(Debug, Serialize, Deserialize)]
/// struct M(u32, String);
/// impl Message for TestMsg {}
/// ```
pub trait Message: Serialize + DeserializeOwned {}

pub trait Middleman: Sized {
	/// Create a new Middleman object protecting the given TcpStream.
	/// This object should already be connected and should have the desired socket options set.
	fn new(TcpStream) -> Self;

	/// Destroy the Middleman and expose the TcpStream it was protecting. In addition to returning the socket,
	/// all bytes that were read but not sufficient to build the next message are returned in a vector. 
	fn into_inner(self) -> (TcpStream, Vec<u8>);

	/// Attempt to receive one message from the stream. This call may fail for a number
	/// of reasons enumerated by the type `TryRecvError`. This call will complete without blocking.
	fn try_recv<M: Message>(&mut self) -> Result<M, TryRecvError>;

	/// Receive a single message from the stream. This call will block until a message is ready. The call
	/// will return `Err` with a `FatalError` in the event something goes wrong with reading from the socket,
	/// or something goes wrong with serialization. Type parameter defines which type to receive.
	/// This should match the type sent or the data might be misinterpreted or fail to be deserialized!
	///
	/// # Examples
	///
	/// ```
	/// let mut mm = Middleman(stream);
	/// match mm.try_recv::<MyStruct>() {
	/// 	Ok(msg) => {
	/// 		// use `msg` for something
	/// 	},
	/// 	Err(TryRecvError::ReadNotReady) => (), // message not ready!
	/// 	Err(TryRecvError::Fatal(f)) => {
	/// 		// handle error
	/// 	}
	/// }
	/// ```
	fn recv<M: Message>(&mut self) -> Result<M, FatalError>;

	/// Send one message through the stream.
	///
	/// # Examples
	///
	/// ```
	/// let mut mm = Middleman(stream);
	/// mm.send(MyStruct(5, 0.5))
	/// .expect("Something went wrong!");
	/// ```
	fn send<M: Message>(&mut self, &M) -> Result<(), FatalError>;

	/// Consumes the given iterator over messages, sending them all in sequence into the stream.
	/// The call returns a tuple (a,b) where a gives the number of messages successfully sent, and b
	/// gives the `Result`. It holds that if the result is `Ok`, then all messages were sent.
	/// Also, no message (except the first) will send unless its predecessor in the iterator was sent also.
	///
	/// # Examples
	///
	/// ```
	/// let batch = vec![
	/// 	TestMsg(1, "one".to_owned()),
	/// 	TestMsg(2, "two".to_owned()),
	/// 	TestMsg(3, "three".to_owned()),
	/// 	TestMsg(4, "four".to_owned()),
	/// ];
	/// let (num_sent, result) = middleman.send_all(batch.iter());
 	/// assert_eq!(num_sent, 4);
 	/// result.expect("Something went wrong!");
	fn send_all<'m, I, M>(&'m mut self, I) -> (usize, Result<(), FatalError>)
	where 
		M: Message + 'm,
		I: Iterator<Item = &'m M>,;
}