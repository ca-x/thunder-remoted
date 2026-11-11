use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use thunder_task_rs::task::TaskFilter;
use thunder_task_rs::{auth::ClientCredentials, types::Phase, Client, LocalNas};
use url::Url;

#[derive(Parser)]
#[command(
    name = "thunder-remoted",
    version,
    about = "Remote download controller for Xunlei NAS devices"
)]
struct Cli {
    /// Client id of your Xunlei application (falls back to THUNDER_CLIENT_ID;
    /// defaults to the id embedded in the official NAS client)
    #[arg(long, global = true)]
    client_id: Option<String>,

    /// Client secret (falls back to THUNDER_CLIENT_SECRET; often empty)
    #[arg(long, global = true)]
    client_secret: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Log in with a QR code / verification link and store the session
    Login,
    /// Import tokens from an existing web session you logged into yourself.
    /// Export from pan.xunlei.com devtools: access_token and refresh_token
    /// from localStorage (key `token`), or any token pair you own.
    Import {
        /// access_token from your own logged-in session
        #[arg(long)]
        access_token: String,
        /// refresh_token (enables auto-renewal)
        #[arg(long)]
        refresh_token: Option<String>,
        /// client id used by the session (default: NAS client id)
        #[arg(long)]
        client_id: Option<String>,
    },
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
    /// Talk to a Xunlei NAS client running next to you (no cloud login).
    /// Uses the token the client hands to its own web UI.
    Local {
        /// NAS client base URL (falls back to THUNDER_NAS_URL)
        #[arg(long)]
        base: Option<String>,
        #[command(subcommand)]
        action: LocalAction,
    },
}

#[derive(Subcommand)]
enum LocalAction {
    /// Device, account and disk information
    Info,
    /// Free-tier daily task quota
    Quota,
    /// List tasks
    Tasks {
        #[arg(long)]
        limit: Option<u32>,
    },
    /// Analyze a link before submitting it
    Analyze { url: String },
    /// Create a download task (respects the client's daily quota)
    Add {
        url: String,
        #[arg(short, long)]
        folder: Option<String>,
        #[arg(short, long)]
        name: Option<String>,
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
        Command::Import {
            access_token,
            refresh_token,
            client_id,
        } => {
            import_tokens(
                access_token.clone(),
                refresh_token.clone(),
                client_id.clone(),
            )
            .await
        }
        Command::Devices => devices(client_creds(&cli)?).await,
        Command::Add {
            url,
            device,
            folder,
            name,
        } => {
            add(
                client_creds(&cli)?,
                url.clone(),
                device.clone(),
                folder.clone(),
                name.clone(),
            )
            .await
        }
        Command::Analyze { url } => analyze(client_creds(&cli)?, url.clone()).await,
        Command::List { all } => list(client_creds(&cli)?, *all).await,
        Command::Task { action } => task_action(client_creds(&cli)?, action).await,
        Command::Local { base, action } => local_mode(base.clone(), action).await,
    }
}

async fn local_mode(base: Option<String>, action: &LocalAction) -> Result<()> {
    let base = base
        .or_else(|| std::env::var("THUNDER_NAS_URL").ok())
        .unwrap_or_else(|| "http://127.0.0.1:2345".to_string());
    let nas = LocalNas::connect(Url::parse(&base).context("invalid NAS base URL")?)
        .await
        .context("could not reach the NAS client (is it running and is the URL right?)")?;
    match action {
        LocalAction::Info => {
            println!(
                "{}",
                serde_json::to_string_pretty(&nas.device_info().await?)?
            );
        }
        LocalAction::Quota => {
            let q = nas.quota().await?;
            match q.remaining() {
                Some(left) => println!("{} of {} used today; {left} left", q.usage, q.limit),
                None => println!("unlimited"),
            }
        }
        LocalAction::Tasks { limit } => {
            let tasks = nas.tasks(*limit).await?;
            println!("{:<34} {:<22} {}", "ID", "PHASE", "NAME");
            for t in tasks {
                println!("{:<34} {:<22} {}", t.id, t.phase.as_str(), t.name);
            }
        }
        LocalAction::Analyze { url } => {
            println!(
                "{}",
                serde_json::to_string_pretty(&nas.analyze(url).await?)?
            );
        }
        LocalAction::Add { url, folder, name } => {
            // Report the free-tier wall before it bites.
            if let Ok(q) = nas.quota().await {
                if q.remaining() == Some(0) {
                    bail!(
                        "daily task quota exhausted ({} of {} used). The NAS channel allows {} tasks/day on this account; nothing was submitted.",
                        q.usage, q.limit, q.limit
                    );
                }
            }
            let mut req = thunder_task_rs::types::CreateTaskRequest::new(url)
                .with_parent_folder_id(folder.clone().unwrap_or_default())
                .with_name(name.clone().unwrap_or_default());
            if name.is_none() {
                req = req.with_name(String::new());
            }
            match nas.create_task(req).await {
                Ok(v) => println!("submitted: {}", serde_json::to_string_pretty(&v)?),
                Err(e) => bail!("submission rejected by the client: {e}"),
            }
        }
    }
    Ok(())
}

fn client_creds(cli: &Cli) -> Result<ClientCredentials> {
    let id = cli
        .client_id
        .clone()
        .or_else(|| std::env::var("THUNDER_CLIENT_ID").ok());
    let secret = cli
        .client_secret
        .clone()
        .or_else(|| std::env::var("THUNDER_CLIENT_SECRET").ok());
    match (id, secret) {
        (Some(id), secret) => Ok(ClientCredentials::new(id, secret.unwrap_or_default())),
        // Default: the client id publicly embedded in the official NAS client.
        (None, _) => Ok(ClientCredentials::nas_client()),
    }
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

async fn import_tokens(
    access_token: String,
    refresh_token: Option<String>,
    client_id: Option<String>,
) -> Result<()> {
    let credentials = match client_id {
        Some(id) => ClientCredentials::new(id, String::new()),
        None => ClientCredentials::nas_client(),
    };
    let tokens = thunder_task_rs::auth::TokenSet {
        token_type: "Bearer".into(),
        access_token,
        refresh_token: refresh_token.unwrap_or_default(),
        expires_in: 0,
        extra: Default::default(),
    };
    let session = thunder_task_rs::auth::Session::from_tokens(credentials, tokens);
    let client = Client::from_session(session);
    save_session(&client).await?;
    println!("session imported; verify with `thunder-remoted devices`");
    Ok(())
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
    println!(
        "{:<34} {:<22} {:>10}  {}",
        "ID", "PHASE", "PROGRESS", "NAME"
    );
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
