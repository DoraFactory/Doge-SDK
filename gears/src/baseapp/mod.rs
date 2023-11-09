pub mod ante;
mod baseapp;
pub mod cli;
mod params;

pub use baseapp::*;

use std::fmt;

#[derive(Debug)]
pub enum BaseAppError {
    P2PListernError,
}

impl fmt::Display for BaseAppError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            BaseAppError::P2PListernError => write!(f, "swarm listen error"),
        }
    }
}