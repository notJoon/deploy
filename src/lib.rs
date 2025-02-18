use clap::{Parser, Subcommand};
use std::path::PathBuf;

pub mod analyze;

#[derive(Parser)]
#[command(name = "deploy")]
#[command(author = "")]
#[command(version = "1.0")]
#[command(about = "Analyzes Gno package dependencies and generates ordered code", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Analyze dependencies and show coupling scores
    Analyze {
        /// Path to the Go project directory
        #[arg(value_name = "PROJECT_PATH")]
        path: PathBuf,

        /// Output format (text, json)
        #[arg(short, long, default_value = "text")]
        format: String,

        /// Show detailed metrics for each package
        #[arg(short, long)]
        detailed: bool,
    },
    /// Generate code based on dependency order
    Generate {
        /// Path to the Go project directory
        #[arg(value_name = "PROJECT_PATH")]
        path: PathBuf,

        /// Output directory for generated code
        #[arg(short, long, value_name = "OUTPUT_DIR")]
        output: Option<PathBuf>,

        /// Template to use for code generation
        #[arg(short, long)]
        template: Option<String>,
    },
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Analyze {
            path,
            format,
            detailed,
        } => {
            let mut analyzer = crate::analyze::DependencyAnalyzer::new();

            // Analyze all .go files in the directory
            for entry in walkdir::WalkDir::new(path)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().is_some_and(|ext| ext == "go"))
            {
                analyzer.analyze_file(entry.path())?;
            }

            analyzer.calculate_coupling_scores();

            // Export and print results
            let output = analyzer.export_analysis(&format, detailed)?;
            println!("{}", output);
        }
        Commands::Generate {
            path,
            output,
            template,
        } => {
            println!("Code generation will be implemented in the future.");
            println!("Project path: {:?}", path);
            println!(
                "Output directory: {:?}",
                output.unwrap_or_else(|| PathBuf::from("."))
            );
            println!(
                "Template: {:?}",
                template.unwrap_or_else(|| "default".to_string())
            );
        }
    }

    Ok(())
}
