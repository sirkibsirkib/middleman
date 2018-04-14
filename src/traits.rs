use super::*;

pub trait Message: Serialize + DeserializeOwned {}

pub trait Middleman: Sized {
	fn new(TcpStream) -> Self;
	fn into_inner(self) -> (TcpStream, Vec<u8>);
	fn try_recv<M: Message>(&mut self) -> Result<M, TryRecvError>;
	fn recv<M: Message>(&mut self) -> Result<M, FatalError>;
	fn send<M: Message>(&mut self, m: &M) -> Result<(), FatalError>;
	fn send_all<'m, I, M>(&'m mut self, m_iter: I) -> (usize, Result<(), FatalError>)
	where 
		M: Message + 'm,
		I: Iterator<Item = &'m M>;
}