pub mod config;
pub mod schemas;
pub mod cache;
pub mod python_worker;
pub mod import_graph;
pub mod planner;
pub mod scheduler;
pub mod worker_pool;
pub mod coverage;
pub mod watch;
pub mod sharder;
pub mod event_stream;
pub mod diagnostics;
pub mod security;
pub mod telemetry;

pub fn hello() -> &'static str {
    "veri-core: hello"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hello() {
        assert_eq!(hello(), "veri-core: hello");
    }
}
