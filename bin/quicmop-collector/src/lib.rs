mod cli;
mod collector;
pub mod config;
mod error;
pub mod service;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
