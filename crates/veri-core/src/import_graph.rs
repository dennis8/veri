//! Import graph analysis and dependency tracking
//!
//! This module provides AST-based import analysis for Python files,
//! building import graphs and reverse dependency mappings for impact analysis.

use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

/// Import graph data structure matching imports.graph.json schema
#[derive(Debug, Serialize, Deserialize)]
pub struct ImportsGraph {
    pub version: String,
    pub generated_at: String,
    pub edges: Vec<ImportEdge>,
    pub dynamic_imports: Vec<DynamicImport>,
    pub unresolved_imports: Vec<UnresolvedImport>,
}

/// Import dependency edge
#[derive(Debug, Serialize, Deserialize)]
pub struct ImportEdge {
    pub from_module: String,
    pub to_module: String,
    pub import_type: ImportType,
    pub line: u32,
    pub names: Vec<String>,
    pub alias: Option<String>,
    pub is_conditional: bool,
}

/// Type of import statement
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImportType {
    Import,
    From,
    Relative,
}

/// Dynamic import that couldn't be statically resolved
#[derive(Debug, Serialize, Deserialize)]
pub struct DynamicImport {
    pub from_module: String,
    pub line: u32,
    pub function: DynamicImportFunction,
    pub argument: Option<String>,
    pub reason: String,
}

/// Dynamic import function types
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DynamicImportFunction {
    #[serde(rename = "importlib.import_module")]
    ImportlibImportModule,
    #[serde(rename = "__import__")]
    Import,
    #[serde(rename = "exec")]
    Exec,
    #[serde(rename = "eval")]
    Eval,
}

/// Import that couldn't be resolved to a local module
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnresolvedImport {
    pub from_module: String,
    pub import_name: String,
    pub line: u32,
    pub is_third_party: bool,
    pub is_builtin: bool,
}

/// Reverse dependencies graph matching revdeps.graph.json schema
#[derive(Debug, Serialize, Deserialize)]
pub struct ReverseDepsGraph {
    pub version: String,
    pub generated_at: String,
    pub reverse_deps: HashMap<String, ModuleReverseDeps>,
}

/// Reverse dependencies for a single module
#[derive(Debug, Serialize, Deserialize)]
pub struct ModuleReverseDeps {
    pub direct_dependents: Vec<String>,
    pub transitive_dependents: Vec<String>,
    pub test_dependents: Vec<String>,
    pub uncertain_dependents: Vec<UncertainDependent>,
}

/// Uncertain dependency due to dynamic imports
#[derive(Debug, Serialize, Deserialize)]
pub struct UncertainDependent {
    pub module: String,
    pub reason: String,
    pub confidence: f64,
}

/// Module map data structure matching module.map.json schema
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleMap {
    pub version: String,
    pub generated_at: String,
    pub modules: HashMap<String, ModuleInfo>,
    pub packages: Vec<PackageInfo>,
}

/// Information about a single module
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleInfo {
    pub module_name: String,
    pub is_package: bool,
    pub is_namespace: bool,
    pub parent_package: Option<String>,
    pub relative_path: String,
    pub digest: String,
}

/// Information about a Python package
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageInfo {
    pub name: String,
    pub path: String,
    pub is_namespace: bool,
    pub subpackages: Vec<String>,
}

/// Import graph builder
pub struct ImportGraphBuilder {
    work_dir: PathBuf,
    cache_dir: PathBuf,
    module_map: ModuleMap,
    py_worker: crate::python_worker::PythonWorker,
}

impl ImportGraphBuilder {
    /// Create a new import graph builder
    pub fn new(work_dir: impl Into<PathBuf>, cache_dir: impl Into<PathBuf>) -> Self {
        let work_dir = work_dir.into();
        let cache_dir = cache_dir.into();
        let py_worker = crate::python_worker::PythonWorker::new(&work_dir, &cache_dir);

        Self {
            work_dir,
            cache_dir,
            module_map: ModuleMap::default(),
            py_worker,
        }
    }

    /// Build complete import graph and reverse dependencies
    pub fn build_graphs(&mut self) -> Result<(ImportsGraph, ReverseDepsGraph, ModuleMap)> {
        // First, build the module map
        self.build_module_map()?;

        // Parse imports from all Python files
        let imports_graph = self.parse_imports()?;

        // Build reverse dependencies from the imports graph
        let revdeps_graph = self.build_reverse_deps(&imports_graph)?;

        // Save all graphs to cache
        self.save_graphs(&imports_graph, &revdeps_graph, &self.module_map)?;

        Ok((imports_graph, revdeps_graph, self.module_map.clone()))
    }

    /// Build module map by discovering Python files and packages
    fn build_module_map(&mut self) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let mut modules = HashMap::new();
        let mut packages = Vec::new();
        let mut discovered_packages = HashSet::new();

        // Walk the directory tree to find Python files
        self.walk_python_files(
            &self.work_dir.clone(),
            "",
            &mut modules,
            &mut packages,
            &mut discovered_packages,
        )?;

        self.module_map = ModuleMap {
            version: "0.1.0".to_string(),
            generated_at: now,
            modules,
            packages,
        };

        Ok(())
    }

    /// Recursively walk directories to find Python files and build module map
    fn walk_python_files(
        &self,
        dir: &Path,
        package_prefix: &str,
        modules: &mut HashMap<String, ModuleInfo>,
        packages: &mut Vec<PackageInfo>,
        discovered_packages: &mut HashSet<String>,
    ) -> Result<()> {
        let entries = fs::read_dir(dir)
            .with_context(|| format!("Failed to read directory: {}", dir.display()))?;

        let mut has_init = false;
        let mut subpackages = Vec::new();
        let mut python_files = Vec::new();

        // First pass: collect all entries
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            let file_name = entry.file_name();
            let file_name_str = file_name.to_string_lossy();

            if path.is_file() && file_name_str.ends_with(".py") {
                python_files.push((path, file_name_str.to_string()));
                if file_name_str == "__init__.py" {
                    has_init = true;
                }
            } else if path.is_dir()
                && !file_name_str.starts_with('.')
                && !file_name_str.starts_with("__pycache__")
            {
                // Check if this directory is a Python package
                let init_path = path.join("__init__.py");
                let has_subinit = init_path.exists();

                let subpackage_name = if package_prefix.is_empty() {
                    file_name_str.to_string()
                } else {
                    format!("{}.{}", package_prefix, file_name_str)
                };

                if has_subinit || self.has_python_files(&path)? {
                    subpackages.push(subpackage_name.clone());

                    // Recursively process subpackage
                    self.walk_python_files(
                        &path,
                        &subpackage_name,
                        modules,
                        packages,
                        discovered_packages,
                    )?;
                }
            }
        }

        // Process Python files in this directory
        for (file_path, file_name) in python_files {
            let relative_path = file_path.strip_prefix(&self.work_dir)?;
            let relative_path_str = relative_path.to_string_lossy().replace('\\', "/");

            let module_name = if file_name == "__init__.py" {
                package_prefix.to_string()
            } else {
                let base_name = file_name.strip_suffix(".py").unwrap();
                if package_prefix.is_empty() {
                    base_name.to_string()
                } else {
                    format!("{}.{}", package_prefix, base_name)
                }
            };

            if !module_name.is_empty() {
                let digest = self.calculate_file_digest(&file_path)?;

                modules.insert(
                    relative_path_str.clone(),
                    ModuleInfo {
                        module_name: module_name.clone(),
                        is_package: file_name == "__init__.py",
                        is_namespace: false, // Will be determined later
                        parent_package: if package_prefix.is_empty() {
                            None
                        } else {
                            Some(package_prefix.to_string())
                        },
                        relative_path: relative_path_str,
                        digest,
                    },
                );
            }
        }

        // Register package if this directory represents one
        if !package_prefix.is_empty() && !discovered_packages.contains(package_prefix) {
            packages.push(PackageInfo {
                name: package_prefix.to_string(),
                path: dir
                    .strip_prefix(&self.work_dir)?
                    .to_string_lossy()
                    .replace('\\', "/"),
                is_namespace: !has_init, // PEP 420 namespace package if no __init__.py
                subpackages,
            });
            discovered_packages.insert(package_prefix.to_string());
        }

        Ok(())
    }

    /// Check if a directory contains Python files (for namespace package detection)
    #[allow(clippy::only_used_in_recursion)]
    fn has_python_files(&self, dir: &Path) -> Result<bool> {
        let entries = fs::read_dir(dir)?;
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() && path.extension().is_some_and(|ext| ext == "py") {
                return Ok(true);
            }
            if path.is_dir() && self.has_python_files(&path)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Calculate SHA-256 digest of file content
    fn calculate_file_digest(&self, file_path: &Path) -> Result<String> {
        use sha2::{Digest, Sha256};
        let content = fs::read(file_path)?;
        let mut hasher = Sha256::new();
        hasher.update(&content);
        Ok(format!("{:x}", hasher.finalize()))
    }

    /// Parse imports from all Python files using the Python worker
    fn parse_imports(&self) -> Result<ImportsGraph> {
        // Use Python worker to parse imports via AST
        let result = self.py_worker.parse_imports(&self.module_map)?;
        Ok(result)
    }

    /// Build reverse dependencies graph from imports graph
    fn build_reverse_deps(&self, imports_graph: &ImportsGraph) -> Result<ReverseDepsGraph> {
        let mut reverse_deps: HashMap<String, ModuleReverseDeps> = HashMap::new();

        // Initialize all modules with empty reverse deps
        for module_info in self.module_map.modules.values() {
            reverse_deps.insert(
                module_info.module_name.clone(),
                ModuleReverseDeps {
                    direct_dependents: Vec::new(),
                    transitive_dependents: Vec::new(),
                    test_dependents: Vec::new(),
                    uncertain_dependents: Vec::new(),
                },
            );
        }

        // Build direct dependencies from import edges
        for edge in &imports_graph.edges {
            if let Some(target_deps) = reverse_deps.get_mut(&edge.to_module) {
                if !target_deps.direct_dependents.contains(&edge.from_module) {
                    target_deps.direct_dependents.push(edge.from_module.clone());
                }

                // Check if the dependent is a test module
                if self.is_test_module(&edge.from_module)
                    && !target_deps.test_dependents.contains(&edge.from_module)
                {
                    target_deps.test_dependents.push(edge.from_module.clone());
                }
            }
        }

        // Build transitive dependencies using BFS
        for module_name in reverse_deps.keys().cloned().collect::<Vec<_>>() {
            let transitive = self.compute_transitive_dependents(&module_name, &reverse_deps);
            if let Some(deps) = reverse_deps.get_mut(&module_name) {
                deps.transitive_dependents = transitive;
            }
        }

        // Add uncertain dependencies from dynamic imports
        for dynamic_import in &imports_graph.dynamic_imports {
            // For dynamic imports, we need to be conservative and potentially mark
            // all modules as uncertain dependents
            self.add_uncertain_dependencies(dynamic_import, &mut reverse_deps);
        }

        Ok(ReverseDepsGraph {
            version: "0.1.0".to_string(),
            generated_at: Utc::now().to_rfc3339(),
            reverse_deps,
        })
    }

    /// Compute transitive dependents using breadth-first search
    fn compute_transitive_dependents(
        &self,
        module_name: &str,
        reverse_deps: &HashMap<String, ModuleReverseDeps>,
    ) -> Vec<String> {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        let mut transitive = Vec::new();

        // Start with direct dependents
        if let Some(deps) = reverse_deps.get(module_name) {
            for dependent in &deps.direct_dependents {
                queue.push_back(dependent.clone());
                visited.insert(dependent.clone());
            }
        }

        // BFS to find all transitive dependents
        while let Some(current) = queue.pop_front() {
            transitive.push(current.clone());

            if let Some(deps) = reverse_deps.get(&current) {
                for dependent in &deps.direct_dependents {
                    if !visited.contains(dependent) {
                        visited.insert(dependent.clone());
                        queue.push_back(dependent.clone());
                    }
                }
            }
        }

        transitive.sort();
        transitive.dedup();
        transitive
    }

    /// Add uncertain dependencies for dynamic imports
    fn add_uncertain_dependencies(
        &self,
        dynamic_import: &DynamicImport,
        reverse_deps: &mut HashMap<String, ModuleReverseDeps>,
    ) {
        // For conservative analysis, dynamic imports create uncertainty about
        // which modules might be imported at runtime

        // If we have a static argument, try to resolve it
        if let Some(argument) = &dynamic_import.argument {
            if let Some(target_deps) = reverse_deps.get_mut(argument) {
                let uncertain = UncertainDependent {
                    module: dynamic_import.from_module.clone(),
                    reason: format!("Dynamic import: {}", dynamic_import.reason),
                    confidence: 0.8, // High confidence if we have the argument
                };
                target_deps.uncertain_dependents.push(uncertain);
            }
        } else {
            // No static argument - this could import anything
            // For now, we'll mark this as requiring broader test selection
            // This will be handled by the planner's safety valves
        }
    }

    /// Check if a module is a test module
    fn is_test_module(&self, module_name: &str) -> bool {
        module_name.starts_with("test_")
            || module_name.ends_with("_test")
            || module_name.contains(".test_")
            || module_name.contains(".tests.")
            || module_name.starts_with("tests.")
    }

    /// Save all graphs to cache directory
    fn save_graphs(
        &self,
        imports_graph: &ImportsGraph,
        revdeps_graph: &ReverseDepsGraph,
        module_map: &ModuleMap,
    ) -> Result<()> {
        // Ensure cache directory exists
        fs::create_dir_all(&self.cache_dir)?;

        // Save imports graph
        let imports_path = self.cache_dir.join("imports.graph.json");
        let imports_json = serde_json::to_string_pretty(imports_graph)?;
        fs::write(&imports_path, imports_json)?;

        // Save reverse dependencies graph
        let revdeps_path = self.cache_dir.join("revdeps.graph.json");
        let revdeps_json = serde_json::to_string_pretty(revdeps_graph)?;
        fs::write(&revdeps_path, revdeps_json)?;

        // Save module map
        let module_map_path = self.cache_dir.join("module.map.json");
        let module_map_json = serde_json::to_string_pretty(module_map)?;
        fs::write(&module_map_path, module_map_json)?;

        Ok(())
    }

    /// Load cached graphs if they exist and are valid
    pub fn load_cached_graphs(
        &self,
    ) -> Result<Option<(ImportsGraph, ReverseDepsGraph, ModuleMap)>> {
        let imports_path = self.cache_dir.join("imports.graph.json");
        let revdeps_path = self.cache_dir.join("revdeps.graph.json");
        let module_map_path = self.cache_dir.join("module.map.json");

        if !imports_path.exists() || !revdeps_path.exists() || !module_map_path.exists() {
            return Ok(None);
        }

        // Load and parse all graphs
        let imports_json = fs::read_to_string(&imports_path)?;
        let imports_graph: ImportsGraph = serde_json::from_str(&imports_json)?;

        let revdeps_json = fs::read_to_string(&revdeps_path)?;
        let revdeps_graph: ReverseDepsGraph = serde_json::from_str(&revdeps_json)?;

        let module_map_json = fs::read_to_string(&module_map_path)?;
        let module_map: ModuleMap = serde_json::from_str(&module_map_json)?;

        Ok(Some((imports_graph, revdeps_graph, module_map)))
    }
}

impl Default for ModuleMap {
    fn default() -> Self {
        Self {
            version: "0.1.0".to_string(),
            generated_at: Utc::now().to_rfc3339(),
            modules: HashMap::new(),
            packages: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    #[test]
    fn builds_simple_import_graph() {
        let work_dir = TempDir::new().expect("work dir");
        let cache_dir = TempDir::new().expect("cache dir");

        fs::write(work_dir.path().join("a.py"), "import b\n").unwrap();
        fs::write(work_dir.path().join("b.py"), "def foo():\n    return 42\n").unwrap();

        let py_worker = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../py_worker")
            .canonicalize()
            .unwrap();
        let existing = env::var("PYTHONPATH").unwrap_or_default();
        let new_path = if existing.is_empty() {
            py_worker.to_string_lossy().to_string()
        } else {
            format!("{}:{}", py_worker.display(), existing)
        };
        env::set_var("PYTHONPATH", new_path);

        let mut builder = ImportGraphBuilder::new(work_dir.path(), cache_dir.path());
        let (imports_graph, _, _) = builder.build_graphs().expect("build graphs");

        assert_eq!(imports_graph.edges.len(), 1);
        let edge = &imports_graph.edges[0];
        assert_eq!(edge.from_module, "a");
        assert_eq!(edge.to_module, "b");
        assert!(matches!(edge.import_type, ImportType::Import));
    }
}
