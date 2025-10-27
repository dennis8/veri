use crate::cli::Cli;
use anyhow::Result;
use std::path::Path;
use veri_core::coverage::{CoverageCollector, CoverageConfig, CoverageFormat};

pub struct CoverageService {
    collector: Option<CoverageCollector>,
}

impl CoverageService {
    pub fn new(cli: &Cli, work_dir: &Path, cache_dir: &Path) -> Self {
        if cli.cov || cli.cov_merge_full {
            let config = CoverageConfig {
                enabled: true,
                merge_full: cli.cov_merge_full,
                output_formats: vec![CoverageFormat::Xml, CoverageFormat::Json],
                output_dir: work_dir.join("reports"),
                source_dirs: vec![work_dir.join("src")],
                omit_patterns: vec![
                    "*/tests/*".to_string(),
                    "*/test_*".to_string(),
                    "*/__pycache__/*".to_string(),
                    "*/venv/*".to_string(),
                    "*/.venv/*".to_string(),
                ],
            };

            let collector =
                CoverageCollector::new(config, cache_dir.to_path_buf(), work_dir.to_path_buf());

            Self {
                collector: Some(collector),
            }
        } else {
            Self { collector: None }
        }
    }

    pub fn initialize(&self, nodeids: &[String]) -> Result<()> {
        if let Some(collector) = &self.collector {
            collector.initialize_coverage(nodeids)?;
        }

        Ok(())
    }

    pub fn finalize(&self, cli: &Cli, nodeids: &[String]) -> Result<()> {
        if let Some(collector) = &self.collector {
            if cli.cov || cli.cov_merge_full {
                let coverage_map = collector.collect_coverage(nodeids)?;
                collector.save_coverage_map(&coverage_map)?;

                if cli.cov_merge_full {
                    collector.generate_full_report(&coverage_map)?;
                    println!("📊 Generated full coverage report in reports/");
                } else if cli.cov {
                    println!("📊 Incremental coverage data collected and cached");
                }
            }
        }

        Ok(())
    }
}
