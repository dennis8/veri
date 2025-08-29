pub mod config;
pub mod schemas;
pub mod cache;
pub mod python_worker;
pub mod import_graph;
pub mod planner;
pub mod scheduler;
pub mod worker_pool;

#[cfg(test)]
mod schema_tests;

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
