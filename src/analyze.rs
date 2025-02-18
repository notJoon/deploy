use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

use tree_sitter::{Parser, Query, QueryCursor, StreamingIterator};
use tree_sitter_go;

use walkdir::WalkDir;

/// Represents a Go package with its dependencies and coupling metrics.
///
/// The coupling score (instability) is calculated as:
/// I = Ce/(Ca+Ce) where:
///  - Ca = Afferent coupling (incoming dependencies)
///  - Ce = Efferent coupling (outgoing dependencies)
#[derive(Debug, PartialEq)]
struct Package {
    /// Name of the package
    name: String,
    // Set of packages that this package imports
    imports: HashSet<String>,
    /// Instability score (0.0 to 1.0, higher means more unstable)
    coupling_score: f64,
}

/// Analysis result for a single package
#[derive(serde::Serialize)]
struct PackageAnalysis {
    name: String,
    coupling_score: f64,
    imports: Vec<String>,
    metrics: DetailedMetrics,
}

/// Detailed dependency metrics
#[derive(serde::Serialize, Default)]
struct DetailedMetrics {
    afferent_coupling: usize, // incoming dependencies
    efferent_coupling: usize, // outgoing dependencies
    instability: f64,         // instability score
    abstractness: f64,        // TODO
    distance: f64,            // TODO: distance from main sequence
}

/// Analyzes dependencies between Go packages and calculates coupling metrics.
///
/// The analyzer walks through Go source files, extracts package dependencies,
/// and computes various coupling metrics to help identify highly coupled or
/// unstable packages.
#[derive(Default, Debug)]
pub struct DependencyAnalyzer {
    /// Map of package names to their corresponding Package instances
    packages: HashMap<String, Package>,
}

impl DependencyAnalyzer {
    /// Creates a new DependencyAnalyzer instance.
    pub fn new() -> Self {
        Self::default()
    }

    /// Analyzes a single Go source file and extracts its package dependencies.
    ///
    /// Uses tree-sitter to parse the Go source file and extract:
    /// - Package declaration
    /// - Import statements
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the Go source file
    ///
    /// # Returns
    ///
    /// * `Ok(())` if analysis succeeds
    /// * `Err` with a description if any error occurs during analysis
    pub fn analyze_file(&mut self, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let source_code = std::fs::read_to_string(path)?;

        let mut parser = Parser::new();
        let language = tree_sitter_go::LANGUAGE;
        parser.set_language(&language.into())?;

        let tree = parser
            .parse(&source_code, None)
            .ok_or("Failed to parse source code")?;

        // handle group and single import
        let query = Query::new(
            &language.into(),
            r#"
            (package_clause
              (package_identifier) @package)
            
            ; single import
            (import_declaration
              (import_spec 
                (interpreted_string_literal) @import))
            
            ; group import
            (import_declaration
              (import_spec_list
                (import_spec
                  (interpreted_string_literal) @import)))
            "#,
        )?;

        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());

        let mut current_package = String::new();
        let mut imports = HashSet::new();

        while let Some(matched) = matches.next() {
            for capture in matched.captures {
                let capture_text = capture
                    .node
                    .utf8_text(source_code.as_bytes())?
                    .trim_matches('"');

                match query.capture_names()[capture.index as usize] {
                    "package" => {
                        current_package = capture_text.to_string();
                    }
                    "import" => {
                        imports.insert(capture_text.to_string());
                    }
                    _ => {}
                }
            }
        }

        if !current_package.is_empty() {
            self.packages.insert(
                current_package.clone(),
                Package {
                    name: current_package,
                    imports,
                    coupling_score: 0.0,
                },
            );
        }

        Ok(())
    }

    /// Calculates coupling scores for all analyzed packages.
    ///
    /// For each package, computes:
    ///  1. Afferent coupling (Ca) - number of packages that depend on it
    ///  2. Efferent coupling (Ce) - number of packages it depends on
    ///  3. Instability (I) = Ce/(Ca+Ce)
    ///
    /// A higher score (closer to 1.0) indicates that the package is more unstable
    /// and dependent on other packages.
    pub fn calculate_coupling_scores(&mut self) {
        let package_imports: HashMap<String, f64> = self
            .packages
            .keys()
            .map(|name| {
                let afferent = self
                    .packages
                    .values()
                    .filter(|p| p.imports.contains(name))
                    .count() as f64;
                (name.clone(), afferent)
            })
            .collect();

        for package in self.packages.values_mut() {
            let afferent = *package_imports.get(&package.name).unwrap_or(&0.0);
            let efferent = package.imports.len() as f64;

            if (afferent + efferent) > 0.0 {
                package.coupling_score = efferent / (afferent + efferent);
                println!(
                    "{}: {:.2} - {} imports",
                    package.name,
                    package.coupling_score,
                    package.imports.len()
                );
            }
        }
    }

    /// Returns a vector of package references sorted by coupling score in descending order.
    ///
    /// Packages with higher coupling scores (more unstable) appear first in the result.
    pub fn get_sorted_packages(&self) -> Vec<&Package> {
        let mut packages: Vec<&Package> = self.packages.values().collect();

        packages.sort_by(|a, b| {
            b.coupling_score
                .partial_cmp(&a.coupling_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        packages
    }

    /// Generates a deployment order based on topological sorting of package dependencies.
    ///
    /// The implementation uses Kahn's algorithm for topological sorting, which:
    /// 1. Identifies nodes with no incoming edges (packages with no dependencies)
    /// 2. Removes these nodes and their outgoing edges from the graph
    /// 3. Repeats until all nodes are processed or a cycle is detected
    ///
    /// # Returns
    ///
    /// * A vector of package references in deployment order (dependencies first)
    /// * Packages with no dependencies come first, followed by packages that depend on them
    ///
    /// # Warning
    ///
    /// If the dependency graph contains cycles, this function will identify packages
    /// involved in cyclic dependencies and will make a best effort to generate a valid order.
    pub fn generate_deployment_order(&self) -> Vec<&Package> {
        // Create a dependency graph where A imports B means B -> A (B must be deployed before A)
        let mut dependency_count: HashMap<&str, usize> = HashMap::new();
        let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new();

        // Initialize for all packages
        for package in self.packages.values() {
            dependency_count.insert(&package.name, 0);
            dependents.insert(&package.name, vec![]);
        }

        // Count dependencies: if A imports B, A depends on B
        for package in self.packages.values() {
            let dependent_name = &package.name;

            // For each import, register it as a dependency of the current package
            for dependency in &package.imports {
                if self.packages.contains_key(dependency) {
                    // This package depends on the imported package
                    *dependency_count.entry(dependent_name).or_insert(0) += 1;

                    // The imported package has this package as a dependent
                    dependents
                        .entry(dependency)
                        .or_insert_with(Vec::new)
                        .push(dependent_name);
                }
            }
        }

        // Start with packages that have no dependencies
        let mut queue: VecDeque<&str> = dependency_count
            .iter()
            .filter(|(_, count)| **count == 0)
            .map(|(&name, _)| name)
            .collect();

        let mut result = Vec::new();

        while let Some(package_name) = queue.pop_front() {
            if let Some(package) = self.packages.get(package_name) {
                result.push(package);
            }

            // For all packages that depend on this one
            if let Some(deps) = dependents.get(package_name) {
                for &dependent in deps {
                    if let Some(count) = dependency_count.get_mut(dependent) {
                        *count -= 1;
                        if *count == 0 {
                            queue.push_back(dependent);
                        }
                    }
                }
            }
        }

        // Check for cycles
        if result.len() < self.packages.len() {
            eprintln!(
                "Warning: Cyclic dependencies detected. Deployment order may not be optimal."
            );

            // Add remaining packages (those involved in cycles)
            for (name, &count) in &dependency_count {
                if count > 0 {
                    if let Some(package) = self.packages.get(*name) {
                        result.push(package);
                    }
                }
            }
        }

        result
    }

    /// Exports analysis results in the specified format
    pub fn export_analysis(
        &self,
        format: &str,
        detailed: bool,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let packages = self.get_sorted_packages();
        let results: Vec<PackageAnalysis> = packages
            .iter()
            .map(|p| {
                let afferent = self
                    .packages
                    .values()
                    .filter(|other| other.imports.contains(&p.name))
                    .count();

                PackageAnalysis {
                    name: p.name.clone(),
                    coupling_score: p.coupling_score,
                    imports: p.imports.iter().cloned().collect(),
                    metrics: DetailedMetrics {
                        afferent_coupling: afferent,
                        efferent_coupling: p.imports.len(),
                        instability: p.coupling_score,
                        abstractness: 0.0, // TODO: Implement
                        distance: 0.0,     // TODO: Implement
                    },
                }
            })
            .collect();

        match format {
            "json" => Ok(serde_json::to_string_pretty(&results)?),
            "text" => {
                let mut output = String::new();
                for result in results {
                    output.push_str(&format!("Package: {}\n", result.name));
                    output.push_str(&format!("Coupling Score: {:.2}\n", result.coupling_score));

                    if detailed {
                        output.push_str(&format!(
                            "Afferent Coupling: {}\n",
                            result.metrics.afferent_coupling
                        ));
                        output.push_str(&format!(
                            "Efferent Coupling: {}\n",
                            result.metrics.efferent_coupling
                        ));
                        output.push_str("Imports:\n");
                        for import in result.imports {
                            output.push_str(&format!("  - {}\n", import));
                        }
                    }
                    output.push_str("\n");
                }
                Ok(output)
            }
            _ => Err("Unsupported output format".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_single_file_analysis() {
        let mut file = NamedTempFile::new().expect("Failed to create temp file");

        let go_source = r#"
            package main
            import (
                "fmt"
                "os"
            )
            func main() {
                fmt.Println("Hello World")
                os.Exit(1)
            }
        "#;

        write!(file, "{}", go_source).unwrap();

        let mut analyzer = DependencyAnalyzer::new();
        analyzer
            .analyze_file(file.path())
            .expect("Failed to analyze temp file");

        assert_eq!(analyzer.packages.len(), 1);

        let pkg_main = analyzer.packages.get("main").unwrap();
        assert_eq!(pkg_main.name, "main");
        assert_eq!(pkg_main.imports.len(), 2);

        let expected_imports: HashSet<String> =
            ["fmt", "os"].iter().map(|s| s.to_string()).collect();
        assert_eq!(pkg_main.imports, expected_imports);
    }

    #[test]
    fn test_coupling_scores() {
        // temp file 1: package "main" -> import "foo"
        let mut file_main = NamedTempFile::new().expect("Failed to create temp file");
        let main_code = r#"
            package main
            import "foo"
        "#;
        write!(file_main, "{}", main_code).unwrap();

        // temp file 2: package "foo" -> import "bar"
        let mut file_foo = NamedTempFile::new().expect("Failed to create temp file");
        let foo_code = r#"
            package foo
            import "bar"
        "#;
        write!(file_foo, "{}", foo_code).unwrap();

        // temp file 3: package "bar" -> no import
        let mut file_bar = NamedTempFile::new().expect("Failed to create temp file");
        let bar_code = r#"
            package bar
        "#;
        write!(file_bar, "{}", bar_code).unwrap();

        // analyze each files and calculate coupling scores
        let mut analyzer = DependencyAnalyzer::new();
        analyzer.analyze_file(file_main.path()).unwrap();
        analyzer.analyze_file(file_foo.path()).unwrap();
        analyzer.analyze_file(file_bar.path()).unwrap();
        analyzer.calculate_coupling_scores();

        // "main" -> import {"foo"}
        // "foo" -> import {"bar"}
        // "bar" -> import {}

        // afferent:
        //   main : (no one imports main) -> Ca=0
        //   foo  : (main imports foo) -> Ca=1
        //   bar  : (foo imports bar) -> Ca=1
        //
        // efferent:
        //   main : imports 1 package -> Ce=1
        //   foo  : imports 1 package -> Ce=1
        //   bar  : imports 0 package -> Ce=0
        //
        // instability I = Ce / (Ca + Ce)
        //   main : I=1/(0+1)=1.0
        //   foo  : I=1/(1+1)=0.5
        //   bar  : I=0/(1+0)=0.0

        let pkg_main = analyzer.packages.get("main").unwrap();
        let pkg_foo = analyzer.packages.get("foo").unwrap();
        let pkg_bar = analyzer.packages.get("bar").unwrap();

        println!("Package main imports: {:?}", pkg_main.imports);
        println!("Package foo imports: {:?}", pkg_foo.imports);
        println!("Package bar imports: {:?}", pkg_bar.imports);

        assert!((pkg_main.coupling_score - 1.0).abs() < f64::EPSILON);
        assert!((pkg_foo.coupling_score - 0.5).abs() < f64::EPSILON);
        assert!((pkg_bar.coupling_score - 0.0).abs() < f64::EPSILON);

        let sorted = analyzer.get_sorted_packages();
        assert_eq!(sorted[0].name, "main"); // 1.0
        assert_eq!(sorted[1].name, "foo"); // 0.5
        assert_eq!(sorted[2].name, "bar"); // 0.0
    }

    #[test]
    fn test_deployment_order() {
        // Create a simple dependency chain: A -> B -> C
        let mut file_a = NamedTempFile::new().expect("Failed to create temp file");
        write!(file_a, "package A\nimport \"B\"").unwrap();

        let mut file_b = NamedTempFile::new().expect("Failed to create temp file");
        write!(file_b, "package B\nimport \"C\"").unwrap();

        let mut file_c = NamedTempFile::new().expect("Failed to create temp file");
        write!(file_c, "package C").unwrap();

        let mut analyzer = DependencyAnalyzer::new();
        analyzer.analyze_file(file_a.path()).unwrap();
        analyzer.analyze_file(file_b.path()).unwrap();
        analyzer.analyze_file(file_c.path()).unwrap();

        analyzer.calculate_coupling_scores();

        // Get deployment order
        let deployment_order = analyzer.generate_deployment_order();

        // Since C has no dependencies, it should be first,
        // followed by B (depends on C), and then A (depends on B)
        assert_eq!(deployment_order.len(), 3);
        assert_eq!(deployment_order[0].name, "C");
        assert_eq!(deployment_order[1].name, "B");
        assert_eq!(deployment_order[2].name, "A");
    }

    /// Tests the topological sort with a more complex dependency graph
    #[test]
    fn test_complex_dependency_graph() {
        // Create a more complex dependency graph:
        // A -> B, C
        // B -> D
        // C -> D
        // D -> (no dependencies)
        // E -> A, D
        let mut file_a = NamedTempFile::new().expect("Failed to create temp file");
        write!(file_a, "package A\nimport (\n\"B\"\n\"C\"\n)").unwrap();

        let mut file_b = NamedTempFile::new().expect("Failed to create temp file");
        write!(file_b, "package B\nimport \"D\"").unwrap();

        let mut file_c = NamedTempFile::new().expect("Failed to create temp file");
        write!(file_c, "package C\nimport \"D\"").unwrap();

        let mut file_d = NamedTempFile::new().expect("Failed to create temp file");
        write!(file_d, "package D").unwrap();

        let mut file_e = NamedTempFile::new().expect("Failed to create temp file");
        write!(file_e, "package E\nimport (\n\"A\"\n\"D\"\n)").unwrap();

        let mut analyzer = DependencyAnalyzer::new();
        analyzer.analyze_file(file_a.path()).unwrap();
        analyzer.analyze_file(file_b.path()).unwrap();
        analyzer.analyze_file(file_c.path()).unwrap();
        analyzer.analyze_file(file_d.path()).unwrap();
        analyzer.analyze_file(file_e.path()).unwrap();

        analyzer.calculate_coupling_scores();

        // Get deployment order
        let deployment_order = analyzer.generate_deployment_order();

        // Verify topological ordering
        assert_eq!(deployment_order.len(), 5);

        // D must come before B, C, A, and E
        let d_pos = deployment_order.iter().position(|p| p.name == "D").unwrap();
        let b_pos = deployment_order.iter().position(|p| p.name == "B").unwrap();
        let c_pos = deployment_order.iter().position(|p| p.name == "C").unwrap();
        let a_pos = deployment_order.iter().position(|p| p.name == "A").unwrap();
        let e_pos = deployment_order.iter().position(|p| p.name == "E").unwrap();

        assert!(d_pos < b_pos);
        assert!(d_pos < c_pos);
        assert!(b_pos < a_pos);
        assert!(c_pos < a_pos);
        assert!(a_pos < e_pos);
    }

    /// Tests that the algorithm handles cyclic dependencies gracefully
    #[test]
    fn test_cyclic_dependencies() {
        // Create a cycle: X -> Y -> X
        let mut file_x = NamedTempFile::new().expect("Failed to create temp file");
        write!(file_x, "package X\nimport \"Y\"").unwrap();

        let mut file_y = NamedTempFile::new().expect("Failed to create temp file");
        write!(file_y, "package Y\nimport \"X\"").unwrap();

        let mut analyzer = DependencyAnalyzer::new();
        analyzer.analyze_file(file_x.path()).unwrap();
        analyzer.analyze_file(file_y.path()).unwrap();

        analyzer.calculate_coupling_scores();

        // Even with a cycle, it should return all packages
        let deployment_order = analyzer.generate_deployment_order();
        assert_eq!(deployment_order.len(), 2);

        // Order doesn't matter as much with cycles, just make sure both are included
        let has_x = deployment_order.iter().any(|p| p.name == "X");
        let has_y = deployment_order.iter().any(|p| p.name == "Y");
        assert!(has_x);
        assert!(has_y);
    }
}
