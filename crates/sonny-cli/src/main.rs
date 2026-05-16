use std::path::PathBuf;

use clap::{Parser, Subcommand};
use memory_runtime::store::connection::Database;
use memory_runtime::store::traits::{ObservationStore, RawMemoryStore};
use memory_runtime::store::observation_store::SqliteObservationStore;
use memory_runtime::store::raw_memory_store::SqliteRawMemoryStore;
use memory_runtime::pipeline::ingest::detect_and_parse;

#[derive(Parser)]
#[command(name = "sonny", about = "Memory Runtime for Coding Agents")]
struct Cli {
    /// Database path
    #[arg(long, env = "SONNY_DB", default_value = "sonny.db")]
    db: PathBuf,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize the database
    Init,

    /// Ingest a session file
    IngestSession {
        /// Path to session file
        path: PathBuf,

        /// Workspace ID
        #[arg(long, default_value = "default")]
        workspace: String,
    },

    /// List observations
    ListObservations {
        /// Filter by status
        #[arg(long)]
        status: Option<String>,

        /// Workspace ID
        #[arg(long, default_value = "default")]
        workspace: String,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init => {
            let db = Database::open(&cli.db).expect("Failed to initialize database");
            println!("Database initialized at {}", cli.db.display());
            drop(db);
        }
        Commands::IngestSession { path, workspace } => {
            let content = std::fs::read_to_string(&path)
                .expect("Failed to read session file");
            let filename = path.file_name().unwrap().to_string_lossy().to_string();
            let source_ref = path.to_string_lossy().to_string();

            let memories = detect_and_parse(&content, &filename, &workspace, &source_ref)
                .expect("Failed to parse session");

            let db = Database::open(&cli.db).expect("Failed to open database");
            let store = SqliteRawMemoryStore::new(db.conn);

            let count = memories.len();
            for m in &memories {
                store.insert(m).expect("Failed to insert raw memory");
            }
            println!("Ingested {count} raw memory records from {filename}");
        }
        Commands::ListObservations { status, workspace } => {
            let db = Database::open(&cli.db).expect("Failed to open database");
            let store = SqliteObservationStore::new(db.conn);

            let observations = store.list_by_workspace(&workspace, status.as_deref())
                .expect("Failed to list observations");

            if observations.is_empty() {
                println!("No observations found.");
                return;
            }

            for obs in &observations {
                println!(
                    "[{}] {} {} {} (confidence: {:.2}, source: {})",
                    obs.status.as_str(),
                    obs.subject_text,
                    obs.predicate,
                    obs.object_text.as_deref().unwrap_or("-"),
                    obs.confidence,
                    obs.source_type.as_str(),
                );
                if let Some(evidence) = &obs.evidence_text {
                    let truncated: String = evidence.chars().take(100).collect();
                    println!("  evidence: {truncated}");
                }
            }
            println!("\n{} observation(s)", observations.len());
        }
    }
}
