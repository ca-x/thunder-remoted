use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use thunder_task_rs::task::TaskFilter;
use thunder_task_rs::{auth::ClientCredentials, types::Phase, Client};

#[derive(Parser)]
#[command(name = "thunder-remoted", version, about = "Remote download controller for Xunlei NAS devices")]
struct Cli {
    /// Client id of your Xunlei application (falls back to THUNDER_CLIENT_ID)
    #[arg(long, global = true)]
    client_id: Option<String>,

    /// Client secret (falls back to THUNDER_CLIENT_SECRET)
    #[arg(long, global = true)]
    client_secret: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Log in with a QR code / verification link and store the session
    Login,
    /// List devices authorized for the account
    Devices,
    /// Create a download task on a device
    Add {
        /// magnet / http(s) / thunder link
        url: String,
        /// device id or unique name substring (interactive pick when omitted)
        #[arg(short, long)]
        device: Option<String>,
        /// target folder id inside the device space (optional)
        #[arg(short, long)]
        folder: Option<String>,
        /// display name (optional)
        #[arg(short, long)]
        name: Option<String>,
    },
    /// Preview what a link contains before submitting
    Analyze {
        /// magnet / http(s) / thunder link
        url: String,
    },
    /// List tasks (default: pending+running)
    List {
        /// show completed and errored tasks too
        #[arg(long)]
        all: bool,
    },
    /// Pause / resume / delete tasks
    Task {
        #[command(subcommand)]
        action: TaskAction,
    },
}

#[derive(Subcommand)]
enum TaskAction {
    Pause { id: String },
    Resume { id: String },
    Delete { ids: Vec<String> },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match &cli.command {
        Command::Login => login(client_creds(&cli)?).await,
        Command::Devices => devices(client_creds(&cli)?).await,
        Command::Add {
            url,
            device,
            folder,
            name,
        } => add(client_creds(&cli)?, url.clone(), device.clone(), folder.clone(), name.clone()).await,
        Command::Analyze { url } => analyze(client_creds(&cli)?, url.clone()).await,
        Command::List { all } => list(client_creds(&cli)?, *all).await,
        Command::Task { action } => task_action(client_creds(&cli)?, action).await,
    }
}

fn client_creds(cli: &Cli) -> Result<ClientCredentials> {
    let id = cli
        .client_id
        .clone()
        .or_else(|| std::env::var("THUNDER_CLIENT_ID").ok())
        .context("missing client id: pass --client-id or set THUNDER_CLIENT_ID")?;
    let secret = cli
        .client_secret
        .clone()
        .or_else(|| std::env::var("THUNDER_CLIENT_SECRET").ok())
        .context("missing client secret: pass --client-secret or set THUNDER_CLIENT_SECRET")?;
    Ok(ClientCredentials::new(id, secret))
}

fn session_path() -> Result<std::path::PathBuf> {
    let dir = dirs::config_dir()
        .context("cannot resolve config dir")?
        .join("thunder-remoted");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("session.json"))
}

async fn load_client(cred: ClientCredentials) -> Result<Client> {
    let path = session_path()?;
    if path.exists() {
        let text = std::fs::read_to_string(&path)?;
        if let Ok(session) = serde_json::from_str::<thunder_task_rs::auth::Session>(&text) {
            if session.credentials.client_id == cred.client_id {
                return Ok(Client::from_session(session));
            }
        }
    }
    bail!("not logged in — run `thunder-remoted login` first")
}

async fn save_session(client: &Client) -> Result<()> {
    if let Some(session) = client.session() {
        let path = session_path()?;
        std::fs::write(&path, serde_json::to_string_pretty(session)?)?;
        println!("session saved to {}", path.display());
    }
    Ok(())
}

async fn login(cred: ClientCredentials) -> Result<()> {
    let mut client = Client::new(cred);
    let session = client
        .login_interactive()
        .await
        .context("device-flow login failed")?;
    println!(
        "\nlogged in: token_type={} expires_in={}s",
        session.tokens.token_type, session.tokens.expires_in
    );
    save_session(&client).await
}

async fn devices(cred: ClientCredentials) -> Result<()> {
    let mut client = load_client(cred).await?;
    let devs = client.devices().await?;
    println!("{:<38} {:<24} {}", "ID", "PLATFORM", "NAME");
    for d in devs {
        println!(
            "{:<38} {:<24} {}",
            d.id,
            d.platform.unwrap_or_default(),
            d.name
        );
        if !d.space.is_empty() {
            println!("    space: {}", d.space);
        }
    }
    Ok(())
}

fn pick_device<'a>(
    devs: &'a [thunder_task_rs::types::Device],
    filter: &Option<String>,
) -> Result<&'a thunder_task_rs::types::Device> {
    match filter {
        Some(f) => {
            let f = f.to_lowercase();
            devs.iter()
                .find(|d| d.id.to_lowercase() == f || d.name.to_lowercase().contains(&f))
                .with_context(|| format!("device not found: {f}"))
        }
        None => {
            for (i, d) in devs.iter().enumerate() {
                println!(
                    "[{}] {} ({}) {}",
                    i,
                    d.name,
                    d.platform.clone().unwrap_or_default(),
                    d.id
                );
            }
            let mut line = String::new();
            std::io::stdin()
                .read_line(&mut line)
                .context("failed to read selection")?;
            let idx: usize = line.trim().parse().context("invalid number")?;
            devs.get(idx).context("index out of range")
        }
    }
}

async fn add(
    cred: ClientCredentials,
    url: String,
    device: Option<String>,
    folder: Option<String>,
    name: Option<String>,
) -> Result<()> {
    let mut client = load_client(cred).await?;
    let devs = client.devices().await?;
    if devs.is_empty() {
        bail!("no authorized devices found on the account");
    }
    let dev = pick_device(&devs, &device)?;
    let req = thunder_task_rs::types::CreateTaskRequest::new(&url)
        .with_space(dev.space.clone())
        .with_parent_folder_id(folder.clone().unwrap_or_default())
        .with_name(name.clone().unwrap_or_default());
    let task = client.create_task(req).await?;
    println!(
        "task created on \"{}\": id={} phase={}",
        dev.name,
        task.id,
        task.phase.as_str()
    );
    Ok(())
}

async fn analyze(cred: ClientCredentials, url: String) -> Result<()> {
    let mut client = load_client(cred).await?;
    let info = client.analyze_link(&url, "").await?;
    println!("{}", serde_json::to_string_pretty(&info)?);
    Ok(())
}

async fn list(cred: ClientCredentials, all: bool) -> Result<()> {
    let mut client = load_client(cred).await?;
    let filter = if all {
        TaskFilter::default()
    } else {
        TaskFilter {
            phases: vec![Phase::Running, Phase::Pending],
            ..TaskFilter::default()
        }
    };
    let tasks = client.tasks(filter).await?;
    println!("{:<34} {:<22} {:>10}  {}", "ID", "PHASE", "PROGRESS", "NAME");
    for t in tasks {
        let prog = t
            .progress
            .map(|p| format!("{p:.1}%"))
            .unwrap_or_else(|| "-".into());
        println!(
            "{:<34} {:<22} {:>10}  {}",
            t.id,
            t.phase.as_str(),
            prog,
            t.name
        );
    }
    Ok(())
}

async fn task_action(cred: ClientCredentials, action: &TaskAction) -> Result<()> {
    let mut client = load_client(cred).await?;
    match action {
        TaskAction::Pause { id } => {
            client.pause_task(id).await?;
            println!("paused {id}");
        }
        TaskAction::Resume { id } => {
            client.resume_task(id).await?;
            println!("resumed {id}");
        }
        TaskAction::Delete { ids } => {
            let owned: Vec<String> = ids.clone();
            client.delete_tasks(&owned).await?;
            println!("deleted {} task(s)", owned.len());
        }
    }
    Ok(())
}
