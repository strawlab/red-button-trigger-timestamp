#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "collections")]
extern crate collections;

#[macro_use]
extern crate serde_derive;
extern crate serde;

#[cfg(not(feature = "std"))]
extern crate core as std;

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone, Copy)]
#[cfg_attr(feature = "print-defmt", derive(defmt::Format))]
pub enum FromDevice {
    Pong(u64),
    Trigger(u64),
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone, Copy)]
#[cfg_attr(feature = "print-defmt", derive(defmt::Format))]
pub enum ToDevice {
    Ping,
}
