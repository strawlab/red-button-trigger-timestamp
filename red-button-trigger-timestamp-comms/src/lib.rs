#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "alloc")]
extern crate alloc;
use serde::{Deserialize, Serialize};

pub const COMMS_NAME: &[u8; 11] = b"triggertime";
pub const COMM_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "print-defmt", derive(defmt::Format))]
pub struct VersionResponse {
    pub name: [u8; 11],
    pub version: u16,
}

impl Default for VersionResponse {
    fn default() -> Self {
        Self {
            name: *COMMS_NAME,
            version: COMM_VERSION,
        }
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone)]
#[cfg_attr(feature = "print-defmt", derive(defmt::Format))]
pub enum FromDevice {
    Pong(u64),
    Trigger(u64),
    VersionResponse(VersionResponse),
}

#[derive(Debug, PartialEq, Serialize, Deserialize, Clone)]
#[cfg_attr(feature = "print-defmt", derive(defmt::Format))]
pub enum ToDevice {
    Ping,
    VersionRequest,
}
