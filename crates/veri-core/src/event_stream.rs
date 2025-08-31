use anyhow::Result;
use chrono::{DateTime, Utc};
use log::debug;
use serde_json;
use std::fs::OpenOptions;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

/// Event types for JSONL stream (as per schema)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type")]
pub enum EventType {
    #[serde(rename = "start")]
    Start {
        timestamp: DateTime<Utc>,
        run_id: String,
        total_tests: u32,
        selected_tests: u32,
        workers: u32,
        strategy: String,
    },
    #[serde(rename = "plan")]
    Plan {
        timestamp: DateTime<Utc>,
        run_id: String,
        shard_id: Option<u32>,
        selected_nodeids: Vec<String>,
        broaden_reason: Option<String>,
        estimated_duration: f64,
    },
    #[serde(rename = "case")]
    Case {
        timestamp: DateTime<Utc>,
        run_id: String,
        shard_id: Option<u32>,
        worker_id: Option<String>,
        nodeid: String,
        outcome: TestOutcome,
        duration: f64,
        message: Option<String>,
    },
    #[serde(rename = "summary")]
    Summary {
        timestamp: DateTime<Utc>,
        run_id: String,
        shard_id: Option<u32>,
        total_duration: f64,
        tests_run: u32,
        tests_passed: u32,
        tests_failed: u32,
        tests_skipped: u32,
        tests_error: u32,
        exit_code: i32,
    },
    #[serde(rename = "log")]
    Log {
        timestamp: DateTime<Utc>,
        run_id: String,
        level: LogLevel,
        message: String,
        context: Option<serde_json::Value>,
    },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TestOutcome {
    Passed,
    Failed,
    Skipped,
    Error,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Debug,
    Info,
    Warning,
    Error,
}

/// Event stream writer for CI integration
pub struct EventStream {
    writer: Option<BufWriter<std::fs::File>>,
    run_id: String,
    shard_id: Option<u32>,
}

impl EventStream {
    /// Create a new event stream
    pub fn new(run_id: String) -> Self {
        Self {
            writer: None,
            run_id,
            shard_id: None,
        }
    }

    /// Create event stream with shard context
    pub fn with_shard(run_id: String, shard_id: u32) -> Self {
        Self {
            writer: None,
            run_id,
            shard_id: Some(shard_id),
        }
    }

    /// Initialize file output for events
    pub fn init_file(&mut self, output_path: &Path) -> Result<()> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(output_path)?;

        self.writer = Some(BufWriter::new(file));
        debug!(
            "Initialized event stream output to: {}",
            output_path.display()
        );
        Ok(())
    }

    /// Emit an event to the stream
    pub fn emit(&mut self, event: EventType) -> Result<()> {
        let event_json = serde_json::to_string(&event)?;

        if let Some(writer) = &mut self.writer {
            writeln!(writer, "{}", event_json)?;
            writer.flush()?;
        }

        debug!("Emitted event: {}", event_json);
        Ok(())
    }

    /// Emit start event
    pub fn emit_start(
        &mut self,
        total_tests: u32,
        selected_tests: u32,
        workers: u32,
        strategy: &str,
    ) -> Result<()> {
        self.emit(EventType::Start {
            timestamp: Utc::now(),
            run_id: self.run_id.clone(),
            total_tests,
            selected_tests,
            workers,
            strategy: strategy.to_string(),
        })
    }

    /// Emit plan event
    pub fn emit_plan(
        &mut self,
        selected_nodeids: Vec<String>,
        broaden_reason: Option<String>,
        estimated_duration: f64,
    ) -> Result<()> {
        self.emit(EventType::Plan {
            timestamp: Utc::now(),
            run_id: self.run_id.clone(),
            shard_id: self.shard_id,
            selected_nodeids,
            broaden_reason,
            estimated_duration,
        })
    }

    /// Emit test case result
    pub fn emit_case(
        &mut self,
        worker_id: Option<String>,
        nodeid: String,
        outcome: TestOutcome,
        duration: f64,
        message: Option<String>,
    ) -> Result<()> {
        self.emit(EventType::Case {
            timestamp: Utc::now(),
            run_id: self.run_id.clone(),
            shard_id: self.shard_id,
            worker_id,
            nodeid,
            outcome,
            duration,
            message,
        })
    }

    /// Emit summary event
    #[allow(clippy::too_many_arguments)]
    pub fn emit_summary(
        &mut self,
        total_duration: f64,
        tests_run: u32,
        tests_passed: u32,
        tests_failed: u32,
        tests_skipped: u32,
        tests_error: u32,
        exit_code: i32,
    ) -> Result<()> {
        self.emit(EventType::Summary {
            timestamp: Utc::now(),
            run_id: self.run_id.clone(),
            shard_id: self.shard_id,
            total_duration,
            tests_run,
            tests_passed,
            tests_failed,
            tests_skipped,
            tests_error,
            exit_code,
        })
    }

    /// Emit log event
    pub fn emit_log(
        &mut self,
        level: LogLevel,
        message: String,
        context: Option<serde_json::Value>,
    ) -> Result<()> {
        self.emit(EventType::Log {
            timestamp: Utc::now(),
            run_id: self.run_id.clone(),
            level,
            message,
            context,
        })
    }

    /// Flush and close the stream
    pub fn close(&mut self) -> Result<()> {
        if let Some(mut writer) = self.writer.take() {
            writer.flush()?;
        }
        Ok(())
    }
}

impl Drop for EventStream {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

/// Generate a unique run ID for this test execution
pub fn generate_run_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let random_suffix: u32 = rand::random();
    format!("veri-{}-{:08x}", timestamp, random_suffix)
}

/// CI reporting utilities
pub struct CIReporter {
    event_stream: Option<EventStream>,
    junit_writer: Option<JUnitWriter>,
}

impl CIReporter {
    pub fn new(run_id: String) -> Self {
        Self {
            event_stream: Some(EventStream::new(run_id)),
            junit_writer: None,
        }
    }

    pub fn with_shard(run_id: String, shard_id: u32) -> Self {
        Self {
            event_stream: Some(EventStream::with_shard(run_id, shard_id)),
            junit_writer: None,
        }
    }

    /// Initialize JSONL output
    pub fn init_jsonl(&mut self, output_path: &Path) -> Result<()> {
        if let Some(stream) = &mut self.event_stream {
            stream.init_file(output_path)?;
        }
        Ok(())
    }

    /// Initialize JUnit XML output
    pub fn init_junit(&mut self, output_path: &Path) -> Result<()> {
        self.junit_writer = Some(JUnitWriter::new(output_path.to_path_buf()));
        Ok(())
    }

    /// Get mutable reference to event stream
    pub fn event_stream(&mut self) -> Option<&mut EventStream> {
        self.event_stream.as_mut()
    }

    /// Get mutable reference to JUnit writer
    pub fn junit_writer(&mut self) -> Option<&mut JUnitWriter> {
        self.junit_writer.as_mut()
    }

    /// Finalize all reports
    pub fn finalize(&mut self) -> Result<()> {
        if let Some(stream) = &mut self.event_stream {
            stream.close()?;
        }
        if let Some(writer) = &mut self.junit_writer {
            writer.finalize()?;
        }
        Ok(())
    }
}

/// Simple JUnit XML writer
pub struct JUnitWriter {
    output_path: PathBuf,
    test_cases: Vec<JUnitTestCase>,
    suite_start_time: std::time::Instant,
}

#[derive(Debug, Clone)]
pub struct JUnitTestCase {
    pub name: String,
    pub classname: String,
    pub time: f64,
    pub result: JUnitResult,
}

#[derive(Debug, Clone)]
pub enum JUnitResult {
    Passed,
    Failed { message: String, details: String },
    Skipped { message: String },
    Error { message: String, details: String },
}

impl JUnitWriter {
    pub fn new(output_path: PathBuf) -> Self {
        Self {
            output_path,
            test_cases: Vec::new(),
            suite_start_time: std::time::Instant::now(),
        }
    }

    pub fn add_test_case(&mut self, test_case: JUnitTestCase) {
        self.test_cases.push(test_case);
    }

    pub fn finalize(&self) -> Result<()> {
        let total_time = self.suite_start_time.elapsed().as_secs_f64();
        let total_tests = self.test_cases.len();
        let failures = self
            .test_cases
            .iter()
            .filter(|tc| matches!(tc.result, JUnitResult::Failed { .. }))
            .count();
        let errors = self
            .test_cases
            .iter()
            .filter(|tc| matches!(tc.result, JUnitResult::Error { .. }))
            .count();
        let skipped = self
            .test_cases
            .iter()
            .filter(|tc| matches!(tc.result, JUnitResult::Skipped { .. }))
            .count();

        let mut xml = String::new();
        xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        xml.push_str(&format!(
            "<testsuite name=\"veri\" tests=\"{}\" failures=\"{}\" errors=\"{}\" skipped=\"{}\" time=\"{:.3}\">\n",
            total_tests, failures, errors, skipped, total_time
        ));

        for test_case in &self.test_cases {
            xml.push_str(&format!(
                "  <testcase name=\"{}\" classname=\"{}\" time=\"{:.3}\"",
                escape_xml(&test_case.name),
                escape_xml(&test_case.classname),
                test_case.time
            ));

            match &test_case.result {
                JUnitResult::Passed => {
                    xml.push_str(" />\n");
                }
                JUnitResult::Failed { message, details } => {
                    xml.push_str(">\n");
                    xml.push_str(&format!(
                        "    <failure message=\"{}\">{}</failure>\n",
                        escape_xml(message),
                        escape_xml(details)
                    ));
                    xml.push_str("  </testcase>\n");
                }
                JUnitResult::Error { message, details } => {
                    xml.push_str(">\n");
                    xml.push_str(&format!(
                        "    <error message=\"{}\">{}</error>\n",
                        escape_xml(message),
                        escape_xml(details)
                    ));
                    xml.push_str("  </testcase>\n");
                }
                JUnitResult::Skipped { message } => {
                    xml.push_str(">\n");
                    xml.push_str(&format!(
                        "    <skipped message=\"{}\" />\n",
                        escape_xml(message)
                    ));
                    xml.push_str("  </testcase>\n");
                }
            }
        }

        xml.push_str("</testsuite>\n");

        // Ensure parent directory exists
        if let Some(parent) = self.output_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        std::fs::write(&self.output_path, xml)?;
        debug!("Wrote JUnit XML to: {}", self.output_path.display());
        Ok(())
    }
}

fn escape_xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempdir::TempDir;

    #[test]
    fn test_event_stream() {
        let temp_dir = TempDir::new("veri_test").unwrap();
        let output_path = temp_dir.path().join("events.jsonl");

        let mut stream = EventStream::new("test-run-123".to_string());
        stream.init_file(&output_path).unwrap();

        stream.emit_start(10, 5, 2, "timing_based").unwrap();
        stream
            .emit_case(
                Some("worker-1".to_string()),
                "test::example".to_string(),
                TestOutcome::Passed,
                1.5,
                None,
            )
            .unwrap();

        stream.close().unwrap();

        let content = std::fs::read_to_string(&output_path).unwrap();
        assert!(content.contains("\"type\":\"start\""));
        assert!(content.contains("\"type\":\"case\""));
        assert!(content.contains("\"outcome\":\"passed\""));
    }

    #[test]
    fn test_junit_writer() {
        let temp_dir = TempDir::new("veri_test").unwrap();
        let output_path = temp_dir.path().join("junit.xml");

        let mut writer = JUnitWriter::new(output_path.clone());

        writer.add_test_case(JUnitTestCase {
            name: "test_example".to_string(),
            classname: "test.module".to_string(),
            time: 1.5,
            result: JUnitResult::Passed,
        });

        writer.add_test_case(JUnitTestCase {
            name: "test_failure".to_string(),
            classname: "test.module".to_string(),
            time: 0.5,
            result: JUnitResult::Failed {
                message: "assertion failed".to_string(),
                details: "Expected true, got false".to_string(),
            },
        });

        writer.finalize().unwrap();

        let content = std::fs::read_to_string(&output_path).unwrap();
        assert!(content.contains("<testsuite"));
        assert!(content.contains("test_example"));
        assert!(content.contains("<failure"));
    }

    #[test]
    fn test_run_id_generation() {
        let id1 = generate_run_id();
        let id2 = generate_run_id();

        assert!(id1.starts_with("veri-"));
        assert!(id2.starts_with("veri-"));
        assert_ne!(id1, id2);
    }
}
