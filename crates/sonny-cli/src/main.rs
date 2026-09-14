use std::path::PathBuf;
use std::time::Duration;

use clap::{Parser, Subcommand};
use memory_runtime::embed::openai::OpenAiCompatibleEmbeddingProvider;
use memory_runtime::models::scope::PromotionThresholds;
use memory_runtime::pipeline::dream::dream_workspace;
use memory_runtime::pipeline::ingest::detect_and_parse;
use memory_runtime::pipeline::promote::maybe_promote_concept;
use memory_runtime::pipeline::timeline::{current_value, format_history};
use memory_runtime::recall::RecallEngine;
use memory_runtime::store::concept_store::SqliteConceptStore;
use memory_runtime::store::connection::Database;
use memory_runtime::store::embedding_store::SqliteEmbeddingStore;
use memory_runtime::store::observation_store::SqliteObservationStore;
use memory_runtime::store::raw_memory_store::SqliteRawMemoryStore;
use memory_runtime::store::relation_store::SqliteRelationStore;
use memory_runtime::store::timeline_store::SqliteTimelineStore;
use memory_runtime::store::traits::{ConceptStore, ObservationStore, RawMemoryStore, TimelineStore};
use memory_runtime::{config::Settings, models::status::ConceptStatus};

#[derive(Parser)]
#[command(name = "sonny", about = "Memory Runtime for Coding Agents")]
struct Cli {
    /// Database path (overrides Settings when set; default: Settings.db_path)
    #[arg(long, env = "SONNY_DB")]
    db: Option<PathBuf>,

    /// Disable elevated (Domain/Global) visibility in recall
    #[arg(long, global = true, default_value_t = false)]
    no_global: bool,

    /// Domain keys for elevated recall (repeatable)
    #[arg(long, global = true)]
    domain_key: Vec<String>,

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

    /// List concepts (optionally elevated from other workspaces)
    ListConcepts {
        #[arg(long, default_value = "default")]
        workspace: String,

        /// Include Domain/Global concepts from other workspaces
        #[arg(long, default_value_t = false)]
        elevated: bool,
    },

    /// Offline dreaming: entity-neighborhood consolidation
    Dream {
        #[arg(long, default_value = "default")]
        workspace: String,
    },

    /// Promote a concept to Domain/Global when evidence allows
    Promote {
        /// Concept ID
        concept_id: String,

        #[arg(long, default_value = "default")]
        workspace: String,

        /// Domain key when promoting to Domain
        #[arg(long)]
        domain: Option<String>,

        /// Observed cross-project count for this knowledge
        #[arg(long, default_value_t = 1)]
        cross_project: i64,

        /// Known conflict count (blocks promotion when > 0)
        #[arg(long, default_value_t = 0)]
        conflicts: i64,
    },

    /// Compact Search recall (hybrid RRF + graph expansion)
    #[command(name = "recall-compact")]
    RecallCompact {
        /// Query text
        query: String,

        #[arg(long, default_value = "default")]
        workspace: String,

        /// Max tokens for MemoryContext
        #[arg(long, default_value_t = 1500)]
        max_tokens: usize,
    },

    /// Entity–property timeline
    Timeline {
        #[command(subcommand)]
        cmd: TimelineCmd,
    },
}

#[derive(Subcommand)]
enum TimelineCmd {
    /// Show history for one (entity, property)
    History {
        entity: String,
        property: String,
        #[arg(long, default_value = "default")]
        workspace: String,
    },
    /// List all timeline entries for an entity
    Entity {
        entity: String,
        #[arg(long, default_value = "default")]
        workspace: String,
    },
    /// Print the current value for (entity, property)
    Current {
        entity: String,
        property: String,
        #[arg(long, default_value = "default")]
        workspace: String,
    },
}

fn open_db(db: Option<&PathBuf>) -> Result<Database, Box<dyn std::error::Error>> {
    let path = match db {
        Some(p) => p.clone(),
        None => Settings::load().db_path,
    };
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    Ok(Database::open(&path)?)
}

fn build_embedding_provider(
    settings: &Settings,
) -> Result<OpenAiCompatibleEmbeddingProvider, Box<dyn std::error::Error>> {
    let base = if settings.embedding.api_url.is_empty() {
        settings.llm.api_url.clone()
    } else {
        settings.embedding.api_url.clone()
    };
    let key = if settings.embedding.api_key.is_empty() {
        settings.llm.api_key.clone()
    } else {
        settings.embedding.api_key.clone()
    };
    let model = if settings.embedding.model_id.is_empty() {
        "BAAI/bge-m3".to_string()
    } else {
        settings.embedding.model_id.clone()
    };
    Ok(OpenAiCompatibleEmbeddingProvider::new(
        &base,
        &key,
        &model,
        settings.embedding.dim,
        Duration::from_secs(settings.embedding.timeout_secs.max(5)),
    )?)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let include_global = !cli.no_global;
    let db_path = cli.db.clone();
    let domain_keys = cli.domain_key.clone();

    match cli.command {
        Commands::Init => {
            let path = db_path
                .clone()
                .unwrap_or_else(|| Settings::load().db_path);
            let db = open_db(db_path.as_ref())?;
            println!("Database initialized at {}", path.display());
            drop(db);
        }
        Commands::IngestSession { path, workspace } => {
            let content = std::fs::read_to_string(&path)?;
            let filename = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();
            let source_ref = path.to_string_lossy().to_string();

            let memories = detect_and_parse(&content, &filename, &workspace, &source_ref)?;

            let db = open_db(db_path.as_ref())?;
            let store = SqliteRawMemoryStore::new(db.conn.clone());

            let count = memories.len();
            store.insert_batch(&memories)?;
            println!("Ingested {count} raw memory records from {filename}");
        }
        Commands::ListObservations { status, workspace } => {
            let db = open_db(db_path.as_ref())?;
            let store = SqliteObservationStore::new(db.conn.clone());

            let status_enum = status.as_deref().and_then(|s| s.parse().ok());

            if status.is_some() && status_enum.is_none() {
                eprintln!(
                    "Invalid status. Valid values: candidate, fast_stored, confirmed, auto_confirmed, rejected, deprecated, disputed, orphan"
                );
                std::process::exit(1);
            }

            let observations = store.list_by_workspace(&workspace, status_enum)?;

            if observations.is_empty() {
                println!("No observations found.");
                return Ok(());
            }

            for obs in &observations {
                println!(
                    "[{}] {} {} {} (eff. {:.2}, xproj {}, source: {})",
                    obs.status.as_str(),
                    obs.subject_text,
                    obs.predicate,
                    obs.object_text.as_deref().unwrap_or("-"),
                    obs.effective_confidence(),
                    obs.cross_project_count,
                    obs.source_type.as_str(),
                );
            }
            println!("\n{} observation(s)", observations.len());
        }
        Commands::ListConcepts {
            workspace,
            elevated,
        } => {
            let db = open_db(db_path.as_ref())?;
            let store = SqliteConceptStore::new(db.conn.clone());
            let concepts = if elevated {
                store.list_visible_concepts(
                    &workspace,
                    Some(ConceptStatus::Active),
                    &domain_keys,
                    include_global,
                )?
            } else {
                store.list_concepts(&workspace, Some(ConceptStatus::Active))?
            };
            if concepts.is_empty() {
                println!("No active concepts.");
                return Ok(());
            }
            for c in &concepts {
                println!(
                    "[{}{}@{}] {} (conf {:.2}, scope {})",
                    c.concept_id,
                    if c.workspace_id != workspace {
                        " ·foreign"
                    } else {
                        ""
                    },
                    c.workspace_id,
                    c.name,
                    c.confidence,
                    c.lifecycle_scope.as_str(),
                );
            }
            println!("\n{} concept(s)", concepts.len());
        }
        Commands::Dream { workspace } => {
            let db = open_db(db_path.as_ref())?;
            let store = SqliteObservationStore::new(db.conn.clone());
            let report = dream_workspace(&store, &workspace)?;
            println!(
                "Dreaming complete for {workspace}: neighborhoods={}, merges={}, softenings={}, archived={}, marked={}",
                report.neighborhoods, report.merges, report.softenings, report.archived, report.marked
            );
        }
        Commands::Promote {
            concept_id,
            workspace,
            domain,
            cross_project,
            conflicts,
        } => {
            let db = open_db(db_path.as_ref())?;
            let store = SqliteConceptStore::new(db.conn.clone());
            // Ensure concept exists in this workspace (or is already elevated).
            match store.get_concept(&concept_id)? {
                None => {
                    eprintln!("Concept {concept_id} not found");
                    std::process::exit(1);
                }
                Some(c) if c.workspace_id != workspace && c.lifecycle_scope.as_str() == "project" => {
                    eprintln!(
                        "Concept {concept_id} belongs to workspace {} and is project-scoped; promote from that workspace",
                        c.workspace_id
                    );
                    std::process::exit(1);
                }
                Some(_) => {}
            }
            let now = chrono::Utc::now().to_rfc3339();
            let report = maybe_promote_concept(
                &store,
                &concept_id,
                cross_project,
                conflicts,
                &PromotionThresholds::default(),
                domain.as_deref(),
                &now,
            )?;
            match report.to {
                Some(scope) => {
                    println!(
                        "Promoted {concept_id}: {:?} → {} (cross_project={}, merged_alias={:?})",
                        report.from,
                        scope.as_str(),
                        report.cross_project_count,
                        report.merged_alias
                    );
                }
                None => {
                    println!(
                        "No promotion for {concept_id} (from {:?}, cross_project={}, conflicts={}). Thresholds not met or conflicts > 0.",
                        report.from, report.cross_project_count, conflicts
                    );
                }
            }
        }
        Commands::RecallCompact {
            query,
            workspace,
            max_tokens,
        } => {
            let settings = Settings::load();
            let db = open_db(db_path.as_ref())?;
            let concepts = SqliteConceptStore::new(db.conn.clone());
            let embeddings = SqliteEmbeddingStore::new(db.conn.clone());
            let relations = SqliteRelationStore::new(db.conn.clone());
            let provider = build_embedding_provider(&settings)?;

            let engine = RecallEngine::new(&concepts, &embeddings, &provider)
                .with_elevated_visibility(domain_keys.clone(), include_global);
            let ctx = engine
                .compact_recall(&query, &workspace, &relations, max_tokens)
                .await?;
            println!("{}", ctx.to_prompt());
            println!("\n--- tokens ≈ {} ---", ctx.token_count);
        }
        Commands::Timeline { cmd } => {
            let db = open_db(db_path.as_ref())?;
            let tl = SqliteTimelineStore::new(db.conn.clone());
            match cmd {
                TimelineCmd::History {
                    entity,
                    property,
                    workspace,
                } => {
                    let hist = tl.history(&workspace, &entity, &property)?;
                    if hist.is_empty() {
                        println!("No timeline entries for {entity}.{property} in {workspace}");
                        return Ok(());
                    }
                    for line in format_history(&hist) {
                        println!("{line}");
                    }
                }
                TimelineCmd::Entity { entity, workspace } => {
                    let all = tl.history_for_entity(&workspace, &entity)?;
                    if all.is_empty() {
                        println!("No timeline entries for entity {entity} in {workspace}");
                        return Ok(());
                    }
                    for e in &all {
                        let mark = if e.is_current() { "●" } else { "○" };
                        println!(
                            "{mark} {}={} [{}] ({} → {})",
                            e.property,
                            e.value.as_deref().unwrap_or("?"),
                            e.status.as_str(),
                            e.valid_from,
                            e.valid_to.as_deref().unwrap_or("now")
                        );
                    }
                }
                TimelineCmd::Current {
                    entity,
                    property,
                    workspace,
                } => match current_value(&tl, &workspace, &entity, &property)? {
                    Some(v) => println!("{entity}.{property} = {v}"),
                    None => println!("{entity}.{property} has no active value in {workspace}"),
                },
            }
        }
    }

    Ok(())
}
