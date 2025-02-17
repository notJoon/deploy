use std::collections::{HashMap, HashSet};
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

/// Analyzes dependencies between Go packages and calculates coupling metrics.
///
/// The analyzer walks through Go source files, extracts package dependencies,
/// and computes various coupling metrics to help identify highly coupled or
/// unstable packages.
#[derive(Default, Debug)]
struct DependencyAnalyzer {
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
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <go-project-directory>", args[0]);
        std::process::exit(1);
    }

    let mut analyzer = DependencyAnalyzer::new();

    for entry in WalkDir::new(&args[1])
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "go"))
    {
        analyzer.analyze_file(entry.path())?;
    }

    analyzer.calculate_coupling_scores();

    println!("Packages sorted by coupling score (higher score = more unstable):");
    for package in analyzer.get_sorted_packages() {
        println!(
            "{}: {:.2} - {} imports",
            package.name,
            package.coupling_score,
            package.imports.len()
        );
    }

    Ok(())
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
}
