use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "veri")]
#[command(version, about = "Ultra-fast pytest-compatible test runner with impact-aware selection")]
#[command(long_about = "veri is a single-binary, pytest-compatible test runner that uses static analysis\nto run only the tests impacted by your changes, making test feedback instant.")]
pub struct Cli {
    /// Run all tests (full collection + run)
    #[arg(short = 'a', long = "all")]
    pub all: bool,

    /// Watch mode - re-run impacted tests on file changes
    #[arg(short = 'w', long = "watch")]
    pub watch: bool,

    /// Only run tests matching this expression
    #[arg(short = 'k', long = "keyword")]
    pub keyword: Option<String>,

    /// Only run tests matching these markers
    #[arg(short = 'm', long = "marker")]
    pub marker: Option<String>,

    /// Number of parallel workers (default: auto-detect)
    #[arg(long = "workers")]
    pub workers: Option<String>,

    /// Re-run only tests that failed in the last run
    #[arg(long = "last-failed")]
    pub last_failed: bool,

    /// Generate JUnit XML report at given path
    #[arg(long = "junit-xml")]
    pub junit_xml: Option<PathBuf>,

    /// Write JSONL event stream to given path
    #[arg(long = "jsonl")]
    pub jsonl: Option<PathBuf>,

    /// Show detailed explanation of test selection
    #[arg(long = "explain")]
    pub explain: bool,

    /// Test execution engine
    #[arg(long = "engine", value_enum, default_value_t = Engine::Veri)]
    pub engine: Engine,

    /// Stop after first failure
    #[arg(short = 'x', long = "exitfirst")]
    pub exitfirst: bool,

    /// Stop after N failures
    #[arg(long = "maxfail")]
    pub maxfail: Option<u32>,

    /// Increase verbosity (-v, -vv)
    #[arg(short = 'v', long = "verbose", action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Decrease verbosity
    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

    /// Disable output capture
    #[arg(long = "no-capture")]
    pub no_capture: bool,

    /// Enable coverage collection
    #[arg(long = "cov")]
    pub cov: bool,

    /// Merge coverage from all workers for full report
    #[arg(long = "cov-merge-full")]
    pub cov_merge_full: bool,

    /// CI mode flag
    #[arg(long = "ci")]
    pub ci: bool,

    /// Configuration file path
    #[arg(short = 'c', long = "config")]
    pub config: Option<PathBuf>,

    /// Test paths or nodeids
    pub paths: Vec<String>,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Split tests into N shards for CI
    Split {
        /// Number of shards to create
        #[arg(long = "ci")]
        shards: u32,
    },
    /// Run a specific shard in CI
    Shard {
        /// Shard index to run (0-based)
        #[arg(long = "ci")]
        shard_id: u32,
        /// Path to shard manifest
        #[arg(long = "manifest")]
        manifest: Option<PathBuf>,
    },
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Debug)]
pub enum Engine {
    /// Use veri's fast engine (default)
    Veri,
    /// Fall back to pytest for compatibility
    Pytest,
}

impl std::fmt::Display for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Engine::Veri => write!(f, "veri"),
            Engine::Pytest => write!(f, "pytest"),
        }
    }
}

/// Exit codes as defined in the specification
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitCode {
    /// All tests passed
    Success = 0,
    /// Some tests failed
    TestFailure = 1,
    /// Test execution was interrupted
    Interrupted = 2,
    /// Internal error occurred
    InternalError = 3,
    /// Usage error (bad arguments, config, etc.)
    UsageError = 4,
}

impl From<ExitCode> for i32 {
    fn from(code: ExitCode) -> Self {
        code as i32
    }
}