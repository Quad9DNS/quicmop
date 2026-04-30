mod cli;
mod collector;
pub mod config;
mod error;
mod metrics;
mod metrics_exporters;
pub mod service;
mod signal;
mod system_metrics;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
