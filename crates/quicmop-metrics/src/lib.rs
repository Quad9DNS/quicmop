pub mod metrics;
pub mod system_metrics;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
