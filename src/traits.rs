use serde::{
    Serialize, 
    de::DeserializeOwned,
};

/// This marker trait must be implemented for any structure intended for
/// sending using a Middleman send or recv. 
pub trait Message: Serialize + DeserializeOwned {}