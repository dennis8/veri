pub mod cache;
pub mod compatibility;
pub mod config;
pub mod coverage;
pub mod diagnostics;
pub mod event_stream;
pub mod flaky;
pub mod import_graph;
pub mod paths;
pub mod planner;
pub mod python_worker;
pub mod scheduler;
pub mod schemas;
pub mod security;
pub mod sharder;
pub mod telemetry;
pub mod watch;
pub mod worker_pool;

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
