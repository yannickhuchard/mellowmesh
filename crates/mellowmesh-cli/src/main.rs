use clap::{Args, Parser, Subcommand};
use mellowmesh_client::MellowMeshClient;

mod commands;
mod mcp;

#[derive(Parser, Debug, Clone)]
#[command(
    name = "mellowmesh",
    version,
    about = "MellowMesh Intelligent Coordination CLI"
)]
struct Cli {
    #[arg(short, long, default_value_t = 40000)]
    port: u16,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug, Clone)]
enum Commands {
    /// Daemon management
    Daemon(DaemonArgs),

    /// Get daemon status
    Status,

    /// List all distinct topics
    Topics,

    /// Publish a message to a topic
    Publish { topic: String, body: String },

    /// Read messages from a topic or pattern
    Read {
        topic: String,
        #[arg(short, long, default_value_t = 20)]
        limit: usize,
    },

    /// Tail a topic pattern (WebSocket streaming)
    Tail { pattern: String },

    /// Agent management
    Agent(AgentCmd),

    /// Named topic mapping management
    NamedTopic(NamedTopicCmd),

    /// List registered agents
    Agents,

    /// Task management
    Task(TaskCmd),

    /// List all tasks
    Tasks,

    /// Claim an open task
    Claim {
        task_id: String,
        #[arg(short, long)]
        agent: String,
        /// Claim lease duration in seconds (default 600). The claim is
        /// auto-released if it expires without a progress heartbeat.
        #[arg(long)]
        lease_seconds: Option<u64>,
    },

    /// Complete a claimed task
    Complete { task_id: String },

    /// Decision management
    Decision(DecisionCmd),

    /// List all decisions
    Decisions,

    /// Respond to a decision request
    Respond {
        decision_id: String,
        option_id: String,
    },

    /// Display messages in a forum-like view grouped by topic
    Forum { pattern: Option<String> },

    /// Search message history
    Search { query: String },

    /// Trace management
    Trace(TraceCmd),

    /// List all trace sessions
    Traces,

    /// View daemon metrics
    Metrics,

    /// Wiki management
    Wiki(WikiCmd),

    /// Schema management (Contracts)
    Schema(SchemaCmd),

    /// Run a guided two-agent coordination demo on the local fabric
    Demo,

    /// Start Model Context Protocol (MCP) server
    Mcp,
}

#[derive(Args, Debug, Clone)]
struct DaemonArgs {
    #[command(subcommand)]
    action: DaemonAction,
}

#[derive(Subcommand, Debug, Clone)]
enum DaemonAction {
    /// Start the local daemon
    Start,
    /// Stop the local daemon
    Stop {
        /// Force stop immediately by killing the process
        #[arg(short, long)]
        force: bool,
    },
    /// Gracefully stop the daemon and restart it
    Restart,
    /// Stop the daemon and delete the local database
    Clean,
}

#[derive(Args, Debug, Clone)]
struct AgentCmd {
    #[command(subcommand)]
    action: AgentAction,
}

#[derive(Subcommand, Debug, Clone)]
enum AgentAction {
    /// Register a new agent
    Register {
        id: String,
        #[arg(long)]
        owner: String,
        #[arg(long, default_value = "human-piloted")]
        mode: String,
        #[arg(long = "capability", short = 'c')]
        capabilities: Vec<String>,
    },
}

#[derive(Args, Debug, Clone)]
struct NamedTopicCmd {
    #[command(subcommand)]
    action: NamedTopicAction,
}

#[derive(Subcommand, Debug, Clone)]
enum NamedTopicAction {
    /// Register or update a named topic mapping
    Register {
        /// Short human-friendly name (e.g. "Mario Galaxy")
        name: String,
        /// Real target topic path (e.g. "_forum.games.mario galaxy")
        topic: String,
    },
    /// List all registered named topics
    List,
    /// Remove a registered named topic mapping
    Remove {
        /// Short human-friendly name to remove
        name: String,
    },
}

#[derive(Args, Debug, Clone)]
struct TaskCmd {
    #[command(subcommand)]
    action: TaskAction,
}

#[derive(Subcommand, Debug, Clone)]
enum TaskAction {
    /// Create a new task
    Create {
        #[arg(long)]
        title: String,
        #[arg(long = "topic", short = 't')]
        topics: Vec<String>,
        #[arg(long)]
        description: Option<String>,
        #[arg(long = "capability", short = 'c')]
        capabilities: Vec<String>,
        #[arg(long, default_value = "medium")]
        priority: String,
        #[arg(long)]
        created_by: Option<String>,
    },
}

#[derive(Args, Debug, Clone)]
struct DecisionCmd {
    #[command(subcommand)]
    action: DecisionAction,
}

#[derive(Subcommand, Debug, Clone)]
enum DecisionAction {
    /// Create a new decision request
    Create {
        #[arg(long)]
        title: String,
        #[arg(long)]
        question: String,
        #[arg(long)]
        created_by: String,
        #[arg(long)]
        decider: String,
        #[arg(long = "option", short = 'o')]
        options: Vec<String>,
    },
}

#[derive(Args, Debug, Clone)]
pub struct TraceCmd {
    #[command(subcommand)]
    pub action: TraceAction,
}

#[derive(Subcommand, Debug, Clone)]
pub enum TraceAction {
    /// Enable dynamic trace session
    Enable {
        target: String,
        #[arg(long, default_value = "agent")]
        target_type: String,
        #[arg(long, default_value = "cognitive")]
        level: String,
        #[arg(long, default_value = "15m")]
        duration: String,
        #[arg(long)]
        reason: Option<String>,
        #[arg(long, default_value = "human://cli")]
        enabled_by: String,
    },
    /// Disable an active trace session
    Disable { id: String },
}

#[derive(Args, Debug, Clone)]
pub struct WikiCmd {
    #[command(subcommand)]
    pub action: WikiAction,
}

#[derive(Subcommand, Debug, Clone)]
pub enum WikiAction {
    /// List all pages in a wiki namespace
    List {
        #[arg(long, default_value = "default")]
        wiki: String,
        #[arg(long)]
        doc_type: Option<String>,
        #[arg(long)]
        tag: Option<String>,
    },
    /// View a specific wiki page by relative path
    View {
        path: String,
        #[arg(long, default_value = "default")]
        wiki: String,
    },
    /// Search wiki pages using full-text search
    Search {
        query: String,
        #[arg(long, default_value = "default")]
        wiki: String,
        #[arg(long)]
        doc_type: Option<String>,
        #[arg(long)]
        tag: Option<String>,
    },
    /// Sync a wiki namespace with the local filesystem
    Sync {
        #[arg(long, default_value = "default")]
        wiki: String,
    },
}

#[derive(Args, Debug, Clone)]
pub struct SchemaCmd {
    #[command(subcommand)]
    pub action: SchemaAction,
}

#[derive(Subcommand, Debug, Clone)]
pub enum SchemaAction {
    /// Add or update a topic schema contract from a JSON file
    Add {
        /// Topic pattern (e.g. '_artifact.invoice.schema' or '_artifact.invoice.>')
        #[arg(long, short = 't')]
        topic: String,
        /// Schema version string (e.g. 'v1', 'v2', '1.0.0')
        #[arg(long, short = 'v')]
        version: String,
        /// Path to the JSON schema file
        #[arg(long, short = 'f')]
        file: String,
    },
    /// Pause validation for a schema version
    Pause {
        #[arg(long, short = 't')]
        topic: String,
        #[arg(long, short = 'v')]
        version: String,
    },
    /// Resume/activate validation for a schema version
    Resume {
        #[arg(long, short = 't')]
        topic: String,
        #[arg(long, short = 'v')]
        version: String,
    },
    /// Remove/delete a schema version
    Remove {
        #[arg(long, short = 't')]
        topic: String,
        #[arg(long, short = 'v')]
        version: String,
    },
    /// List all registered topic schemas
    List,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Intercept custom protocol launch URLs (e.g. mellowmesh://start)
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 && args[1].starts_with("mellowmesh://") {
        println!("Intercepted Custom Protocol URL: {}", args[1]);
        println!("Automatically launching MellowMesh daemon...");
        return commands::run_daemon_start(40000).await;
    }

    let cli = Cli::parse();

    // Match commands that don't need a running client first
    match &cli.command {
        Commands::Daemon(DaemonArgs {
            action: DaemonAction::Start,
        }) => {
            return commands::run_daemon_start(cli.port).await;
        }
        Commands::Daemon(DaemonArgs {
            action: DaemonAction::Stop { force },
        }) => {
            return commands::run_daemon_stop(cli.port, *force).await;
        }
        Commands::Daemon(DaemonArgs {
            action: DaemonAction::Restart,
        }) => {
            return commands::run_daemon_restart(cli.port).await;
        }
        Commands::Daemon(DaemonArgs {
            action: DaemonAction::Clean,
        }) => {
            return commands::run_daemon_clean(cli.port).await;
        }
        Commands::Status => {
            return commands::run_status(cli.port).await;
        }
        _ => {}
    }

    // Connect client (this will auto-start daemon if not running)
    let client = MellowMeshClient::connect_with_port(cli.port).await?;

    match cli.command {
        Commands::Topics => {
            commands::run_topics(&client).await?;
        }
        Commands::Publish { topic, body } => {
            commands::run_publish(&client, topic, body).await?;
        }
        Commands::Read { topic, limit } => {
            commands::run_read(&client, topic, limit).await?;
        }
        Commands::Tail { pattern } => {
            commands::run_tail(&client, pattern).await?;
        }
        Commands::Agent(AgentCmd {
            action:
                AgentAction::Register {
                    id,
                    owner,
                    mode,
                    capabilities,
                },
        }) => {
            commands::run_agent_register(&client, id, owner, mode, capabilities).await?;
        }
        Commands::NamedTopic(NamedTopicCmd { action }) => match action {
            NamedTopicAction::Register { name, topic } => {
                commands::run_named_topic_register(&client, name, topic).await?;
            }
            NamedTopicAction::List => {
                commands::run_named_topic_list(&client).await?;
            }
            NamedTopicAction::Remove { name } => {
                commands::run_named_topic_remove(&client, &name).await?;
            }
        },
        Commands::Agents => {
            commands::run_agents(&client).await?;
        }
        Commands::Task(TaskCmd {
            action:
                TaskAction::Create {
                    title,
                    topics,
                    description,
                    capabilities,
                    priority,
                    created_by,
                },
        }) => {
            commands::run_task_create(
                &client,
                title,
                topics,
                description,
                capabilities,
                priority,
                created_by,
            )
            .await?;
        }
        Commands::Tasks => {
            commands::run_tasks(&client).await?;
        }
        Commands::Claim {
            task_id,
            agent,
            lease_seconds,
        } => {
            commands::run_claim(&client, &task_id, &agent, lease_seconds).await?;
        }
        Commands::Complete { task_id } => {
            commands::run_complete(&client, &task_id).await?;
        }
        Commands::Decision(DecisionCmd {
            action:
                DecisionAction::Create {
                    title,
                    question,
                    created_by,
                    decider,
                    options,
                },
        }) => {
            commands::run_decision_create(&client, title, question, created_by, decider, options)
                .await?;
        }
        Commands::Decisions => {
            commands::run_decisions(&client).await?;
        }
        Commands::Respond {
            decision_id,
            option_id,
        } => {
            commands::run_respond(&client, &decision_id, &option_id).await?;
        }
        Commands::Forum { pattern } => {
            commands::run_forum(&client, pattern).await?;
        }
        Commands::Search { query } => {
            commands::run_search(&client, query).await?;
        }
        Commands::Trace(TraceCmd { action }) => match action {
            TraceAction::Enable {
                target,
                target_type,
                level,
                duration,
                reason,
                enabled_by,
            } => {
                commands::run_trace_enable(
                    &client,
                    &target_type,
                    &target,
                    &level,
                    &duration,
                    reason,
                    &enabled_by,
                )
                .await?;
            }
            TraceAction::Disable { id } => {
                commands::run_trace_disable(&client, &id).await?;
            }
        },
        Commands::Traces => {
            commands::run_traces(&client).await?;
        }
        Commands::Metrics => {
            commands::run_metrics(&client).await?;
        }
        Commands::Wiki(WikiCmd { action }) => match action {
            WikiAction::List {
                wiki,
                doc_type,
                tag,
            } => {
                commands::run_wiki_list(&client, &wiki, doc_type.as_deref(), tag.as_deref())
                    .await?;
            }
            WikiAction::View { path, wiki } => {
                commands::run_wiki_view(&client, &wiki, &path).await?;
            }
            WikiAction::Search {
                query,
                wiki,
                doc_type,
                tag,
            } => {
                commands::run_wiki_search(
                    &client,
                    &wiki,
                    &query,
                    doc_type.as_deref(),
                    tag.as_deref(),
                )
                .await?;
            }
            WikiAction::Sync { wiki } => {
                commands::run_wiki_sync(&client, &wiki).await?;
            }
        },
        Commands::Schema(SchemaCmd { action }) => match action {
            SchemaAction::Add {
                topic,
                version,
                file,
            } => {
                commands::run_schema_add(&client, &topic, &version, &file).await?;
            }
            SchemaAction::Pause { topic, version } => {
                commands::run_schema_status(&client, &topic, &version, "paused").await?;
            }
            SchemaAction::Resume { topic, version } => {
                commands::run_schema_status(&client, &topic, &version, "active").await?;
            }
            SchemaAction::Remove { topic, version } => {
                commands::run_schema_remove(&client, &topic, &version).await?;
            }
            SchemaAction::List => {
                commands::run_schema_list(&client).await?;
            }
        },
        Commands::Demo => {
            commands::run_demo(&client).await?;
        }
        Commands::Mcp => {
            mcp::run_mcp_server(cli.port).await?;
        }
        _ => unreachable!(),
    }

    Ok(())
}
