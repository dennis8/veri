use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use log::{debug, info, warn};
use sha2::{Sha256, Digest};

/// Coverage map data keyed by file digest
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageMap {
    /// Mapping from file path to coverage data
    pub files: HashMap<String, FileCoverage>,
    /// Metadata about the coverage collection
    pub metadata: CoverageMetadata,
}

/// Coverage data for a single file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileCoverage {
    /// File digest for cache validation
    pub digest: String,
    /// Set of covered line numbers (1-indexed)
    pub covered_lines: Vec<u32>,
    /// Total number of executable lines
    pub total_lines: u32,
    /// Coverage percentage
    pub coverage_percent: f64,
}

/// Metadata about coverage collection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageMetadata {
    /// When this coverage was collected
    pub timestamp: String,
    /// Python version used
    pub python_version: String,
    /// Coverage tool version
    pub coverage_version: String,
    /// Whether this is incremental or full coverage
    pub incremental: bool,
}

/// Configuration for coverage collection
#[derive(Debug, Clone)]
pub struct CoverageConfig {
    /// Enable coverage collection
    pub enabled: bool,
    /// Enable full merge for CI reports
    pub merge_full: bool,
    /// Output format (xml, html, json)
    pub output_formats: Vec<CoverageFormat>,
    /// Output directory for reports
    pub output_dir: PathBuf,
    /// Source directories to include in coverage
    pub source_dirs: Vec<PathBuf>,
    /// Files/patterns to exclude from coverage
    pub omit_patterns: Vec<String>,
}

/// Coverage output formats
#[derive(Debug, Clone, PartialEq)]
pub enum CoverageFormat {
    Xml,
    Html,
    Json,
    Lcov,
}

/// Coverage collector handles incremental coverage tracking
pub struct CoverageCollector {
    config: CoverageConfig,
    cache_dir: PathBuf,
    work_dir: PathBuf,
}

impl CoverageCollector {
    /// Create a new coverage collector
    pub fn new(config: CoverageConfig, cache_dir: PathBuf, work_dir: PathBuf) -> Self {
        Self {
            config,
            cache_dir,
            work_dir,
        }
    }

    /// Initialize coverage collection for selected tests
    pub fn initialize_coverage(&self, selected_tests: &[String]) -> Result<()> {
        if !self.config.enabled {
            return Ok(());
        }

        info!("Initializing incremental coverage for {} tests", selected_tests.len());
        
        // Create coverage configuration for the Python worker
        self.create_coverage_config(selected_tests)?;
        
        Ok(())
    }

    /// Collect coverage data from a test run
    pub fn collect_coverage(&self, test_results: &[String]) -> Result<CoverageMap> {
        if !self.config.enabled {
            return Ok(CoverageMap {
                files: HashMap::new(),
                metadata: self.create_metadata(true),
            });
        }

        debug!("Collecting coverage data from {} test results", test_results.len());
        
        // Read coverage data from Python coverage tool
        let coverage_data = self.read_coverage_data()?;
        
        // Convert to our format
        let coverage_map = self.convert_to_coverage_map(coverage_data)?;
        
        // Merge with existing coverage if incremental
        self.merge_incremental_coverage(coverage_map)
    }

    /// Merge coverage maps using digest-keyed approach
    pub fn merge_coverage_maps(&self, base: &CoverageMap, delta: &CoverageMap) -> Result<CoverageMap> {
        debug!("Merging coverage maps: base={} files, delta={} files", 
               base.files.len(), delta.files.len());
        
        let mut merged = base.clone();
        
        for (file_path, delta_coverage) in &delta.files {
            match merged.files.get(file_path) {
                Some(base_coverage) => {
                    // If digests match, merge line coverage using bitwise OR
                    if base_coverage.digest == delta_coverage.digest {
                        let merged_coverage = self.merge_file_coverage(base_coverage, delta_coverage)?;
                        merged.files.insert(file_path.clone(), merged_coverage);
                    } else {
                        // File changed, use delta coverage (more recent)
                        warn!("File {} changed (digest mismatch), using new coverage", file_path);
                        merged.files.insert(file_path.clone(), delta_coverage.clone());
                    }
                }
                None => {
                    // New file, add delta coverage
                    merged.files.insert(file_path.clone(), delta_coverage.clone());
                }
            }
        }
        
        // Update metadata
        merged.metadata = self.create_metadata(true);
        
        Ok(merged)
    }

    /// Generate full coverage report for CI
    pub fn generate_full_report(&self, coverage_map: &CoverageMap) -> Result<()> {
        if !self.config.merge_full {
            return Ok(());
        }

        info!("Generating full coverage report with {} files", coverage_map.files.len());
        
        // Create output directory
        std::fs::create_dir_all(&self.config.output_dir)
            .context("Failed to create coverage output directory")?;
        
        // Generate reports in requested formats
        for format in &self.config.output_formats {
            match format {
                CoverageFormat::Xml => self.generate_xml_report(coverage_map)?,
                CoverageFormat::Html => self.generate_html_report(coverage_map)?,
                CoverageFormat::Json => self.generate_json_report(coverage_map)?,
                CoverageFormat::Lcov => self.generate_lcov_report(coverage_map)?,
            }
        }
        
        Ok(())
    }

    /// Save coverage map to cache
    pub fn save_coverage_map(&self, coverage_map: &CoverageMap) -> Result<()> {
        let cache_file = self.cache_dir.join("coverage.map.json");
        let json = serde_json::to_string_pretty(coverage_map)
            .context("Failed to serialize coverage map")?;
        
        std::fs::write(&cache_file, json)
            .context("Failed to write coverage map to cache")?;
        
        debug!("Saved coverage map to {}", cache_file.display());
        Ok(())
    }

    /// Load coverage map from cache
    pub fn load_coverage_map(&self) -> Result<Option<CoverageMap>> {
        let cache_file = self.cache_dir.join("coverage.map.json");
        
        if !cache_file.exists() {
            return Ok(None);
        }
        
        let json = std::fs::read_to_string(&cache_file)
            .context("Failed to read coverage map from cache")?;
        
        let coverage_map: CoverageMap = serde_json::from_str(&json)
            .context("Failed to deserialize coverage map")?;
        
        debug!("Loaded coverage map from {}", cache_file.display());
        Ok(Some(coverage_map))
    }

    // Private helper methods

    fn create_coverage_config(&self, _selected_tests: &[String]) -> Result<()> {
        // Create .coveragerc file for Python coverage tool
        let coveragerc_path = self.work_dir.join(".coveragerc");
        let mut config_content = String::new();
        
        config_content.push_str("[run]\n");
        config_content.push_str("branch = True\n");
        config_content.push_str("parallel = True\n");
        config_content.push_str("data_file = .veri/cache/.coverage\n");
        
        if !self.config.source_dirs.is_empty() {
            config_content.push_str("source = ");
            let sources: Vec<String> = self.config.source_dirs
                .iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect();
            config_content.push_str(&sources.join(", "));
            config_content.push('\n');
        }
        
        if !self.config.omit_patterns.is_empty() {
            config_content.push_str("omit = ");
            config_content.push_str(&self.config.omit_patterns.join(", "));
            config_content.push('\n');
        }
        
        config_content.push_str("\n[report]\n");
        config_content.push_str("skip_covered = False\n");
        config_content.push_str("skip_empty = True\n");
        
        std::fs::write(&coveragerc_path, config_content)
            .context("Failed to write coverage configuration")?;
        
        debug!("Created coverage config at {}", coveragerc_path.display());
        Ok(())
    }

    fn read_coverage_data(&self) -> Result<serde_json::Value> {
        // Read from coverage.py's JSON report
        let coverage_json = self.cache_dir.join("coverage.json");
        
        if !coverage_json.exists() {
            // Return empty coverage if no data available
            return Ok(serde_json::json!({
                "files": {},
                "totals": {}
            }));
        }
        
        let json_content = std::fs::read_to_string(&coverage_json)
            .context("Failed to read coverage JSON report")?;
        
        serde_json::from_str(&json_content)
            .context("Failed to parse coverage JSON report")
    }

    fn convert_to_coverage_map(&self, coverage_data: serde_json::Value) -> Result<CoverageMap> {
        let mut files = HashMap::new();
        
        if let Some(file_data) = coverage_data.get("files") {
            if let Some(file_map) = file_data.as_object() {
                for (file_path, file_info) in file_map {
                    if let Some(coverage_info) = self.parse_file_coverage(file_path, file_info)? {
                        files.insert(file_path.clone(), coverage_info);
                    }
                }
            }
        }
        
        Ok(CoverageMap {
            files,
            metadata: self.create_metadata(true),
        })
    }

    fn parse_file_coverage(&self, file_path: &str, file_info: &serde_json::Value) -> Result<Option<FileCoverage>> {
        // Extract covered lines from coverage.py format
        let executed_lines = file_info.get("executed_lines")
            .and_then(|v| v.as_array())
            .map(|lines| {
                lines.iter()
                    .filter_map(|v| v.as_u64())
                    .map(|n| n as u32)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let summary = file_info.get("summary");
        let total_lines = summary
            .and_then(|s| s.get("num_statements"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u32;

        let coverage_percent = summary
            .and_then(|s| s.get("percent_covered"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        // Compute file digest for caching
        let full_path = self.work_dir.join(file_path);
        let digest = if full_path.exists() {
            self.compute_file_digest(&full_path)?
        } else {
            String::new()
        };

        Ok(Some(FileCoverage {
            digest,
            covered_lines: executed_lines,
            total_lines,
            coverage_percent,
        }))
    }

    fn merge_incremental_coverage(&self, new_coverage: CoverageMap) -> Result<CoverageMap> {
        match self.load_coverage_map()? {
            Some(existing_coverage) => {
                self.merge_coverage_maps(&existing_coverage, &new_coverage)
            }
            None => Ok(new_coverage),
        }
    }

    fn merge_file_coverage(&self, base: &FileCoverage, delta: &FileCoverage) -> Result<FileCoverage> {
        // Merge covered lines using set union
        let mut covered_lines = base.covered_lines.clone();
        for &line in &delta.covered_lines {
            if !covered_lines.contains(&line) {
                covered_lines.push(line);
            }
        }
        covered_lines.sort();

        // Recalculate coverage percentage
        let total_lines = std::cmp::max(base.total_lines, delta.total_lines);
        let coverage_percent = if total_lines > 0 {
            (covered_lines.len() as f64 / total_lines as f64) * 100.0
        } else {
            0.0
        };

        Ok(FileCoverage {
            digest: delta.digest.clone(), // Use newer digest
            covered_lines,
            total_lines,
            coverage_percent,
        })
    }

    fn create_metadata(&self, incremental: bool) -> CoverageMetadata {
        CoverageMetadata {
            timestamp: chrono::Utc::now().to_rfc3339(),
            python_version: "3.x".to_string(), // Will be filled by Python worker
            coverage_version: "7.x".to_string(), // Will be filled by Python worker
            incremental,
        }
    }

    fn compute_file_digest(&self, file_path: &Path) -> Result<String> {
        use std::io::Read;
        
        let mut file = std::fs::File::open(file_path)
            .context("Failed to open file for digest computation")?;
        
        let mut hasher = sha2::Sha256::new();
        let mut buffer = [0; 8192];
        
        loop {
            let bytes_read = file.read(&mut buffer)
                .context("Failed to read file for digest computation")?;
            
            if bytes_read == 0 {
                break;
            }
            
            use sha2::Digest;
            hasher.update(&buffer[..bytes_read]);
        }
        
        Ok(format!("{:x}", hasher.finalize()))
    }

    fn generate_xml_report(&self, coverage_map: &CoverageMap) -> Result<()> {
        let xml_path = self.config.output_dir.join("coverage.xml");
        
        // Generate Cobertura XML format for CI tools
        let mut xml_content = String::new();
        xml_content.push_str(r#"<?xml version="1.0" ?>"#);
        xml_content.push('\n');
        xml_content.push_str(r#"<coverage version="1.0" timestamp=""#);
        xml_content.push_str(&format!("{}", chrono::Utc::now().timestamp()));
        xml_content.push_str(r#"">"#);
        xml_content.push('\n');
        
        xml_content.push_str("  <sources>\n");
        for source_dir in &self.config.source_dirs {
            xml_content.push_str(&format!("    <source>{}</source>\n", source_dir.display()));
        }
        xml_content.push_str("  </sources>\n");
        
        xml_content.push_str("  <packages>\n");
        
        // Group files by package (directory)
        let mut packages: HashMap<String, Vec<(&String, &FileCoverage)>> = HashMap::new();
        for (file_path, coverage) in &coverage_map.files {
            let package = Path::new(file_path)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| ".".to_string());
            
            packages.entry(package).or_default().push((file_path, coverage));
        }
        
        for (package_name, files) in packages {
            xml_content.push_str(&format!("    <package name=\"{}\">\n", package_name));
            xml_content.push_str("      <classes>\n");
            
            for (file_path, coverage) in files {
                let class_name = Path::new(file_path)
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                
                xml_content.push_str(&format!(
                    "        <class name=\"{}\" filename=\"{}\" line-rate=\"{:.4}\">\n",
                    class_name, file_path, coverage.coverage_percent / 100.0
                ));
                
                xml_content.push_str("          <lines>\n");
                for &line_num in &coverage.covered_lines {
                    xml_content.push_str(&format!(
                        "            <line number=\"{}\" hits=\"1\"/>\n", line_num
                    ));
                }
                xml_content.push_str("          </lines>\n");
                xml_content.push_str("        </class>\n");
            }
            
            xml_content.push_str("      </classes>\n");
            xml_content.push_str("    </package>\n");
        }
        
        xml_content.push_str("  </packages>\n");
        xml_content.push_str("</coverage>\n");
        
        std::fs::write(&xml_path, xml_content)
            .context("Failed to write XML coverage report")?;
        
        info!("Generated XML coverage report at {}", xml_path.display());
        Ok(())
    }

    fn generate_html_report(&self, _coverage_map: &CoverageMap) -> Result<()> {
        // HTML report generation would be implemented here
        // For now, just create a placeholder
        let html_dir = self.config.output_dir.join("htmlcov");
        std::fs::create_dir_all(&html_dir)?;
        
        let index_path = html_dir.join("index.html");
        std::fs::write(&index_path, "<html><body><h1>Coverage Report</h1><p>HTML report generation not yet implemented</p></body></html>")?;
        
        debug!("Generated placeholder HTML coverage report at {}", html_dir.display());
        Ok(())
    }

    fn generate_json_report(&self, coverage_map: &CoverageMap) -> Result<()> {
        let json_path = self.config.output_dir.join("coverage.json");
        
        let json = serde_json::to_string_pretty(coverage_map)
            .context("Failed to serialize coverage map to JSON")?;
        
        std::fs::write(&json_path, json)
            .context("Failed to write JSON coverage report")?;
        
        info!("Generated JSON coverage report at {}", json_path.display());
        Ok(())
    }

    fn generate_lcov_report(&self, coverage_map: &CoverageMap) -> Result<()> {
        let lcov_path = self.config.output_dir.join("coverage.info");
        
        let mut lcov_content = String::new();
        
        for (file_path, coverage) in &coverage_map.files {
            lcov_content.push_str(&format!("SF:{}\n", file_path));
            
            for &line_num in &coverage.covered_lines {
                lcov_content.push_str(&format!("DA:{},1\n", line_num));
            }
            
            lcov_content.push_str(&format!("LF:{}\n", coverage.total_lines));
            lcov_content.push_str(&format!("LH:{}\n", coverage.covered_lines.len()));
            lcov_content.push_str("end_of_record\n");
        }
        
        std::fs::write(&lcov_path, lcov_content)
            .context("Failed to write LCOV coverage report")?;
        
        info!("Generated LCOV coverage report at {}", lcov_path.display());
        Ok(())
    }
}

impl Default for CoverageConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            merge_full: false,
            output_formats: vec![CoverageFormat::Xml],
            output_dir: PathBuf::from("reports"),
            source_dirs: vec![PathBuf::from("src")],
            omit_patterns: vec![
                "*/tests/*".to_string(),
                "*/test_*".to_string(),
                "*/__pycache__/*".to_string(),
                "*/venv/*".to_string(),
                "*/.venv/*".to_string(),
            ],
        }
    }
}