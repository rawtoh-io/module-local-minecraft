// Bridges a Minecraft Java server to the hub over RCON (built into vanilla,
// Paper and Fabric servers, no plugin needed): the minecraft.* methods become
// console commands, so a Twitch redeem can drop TNT on the streamer.

mod commands;
mod events;
mod rcon;

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use rawtoh_module_sdk::{
    Handler, HubConnection, HubOptions, Identity, MethodFuture, RpcError, enroll,
};
use serde_json::{Value, json};

const IDENTITY_FILE: &str = "identity.json";

#[derive(Parser)]
#[command(
    version,
    about = "Rawtoh Minecraft module: hub methods to RCON commands"
)]
struct Cli {
    #[arg(long, env = "RAWTOH_API_URL", default_value = "https://app.rawtoh.io")]
    rawtoh_api: String,
    #[arg(long, env = "RAWTOH_WS_URL", default_value = "wss://rpc.rawtoh.io")]
    rawtoh_ws: String,
    /// One-shot enrollment token from Module → Instances. Re-enrolls when given.
    #[arg(long, env = "RAWTOH_ENROLL_TOKEN")]
    enroll: Option<String>,
    /// Where identity.json lives (mode 600).
    #[arg(long, env = "STATE_DIR", default_value = "./state")]
    state_dir: PathBuf,

    /// server.properties: rcon.port (enable-rcon=true).
    #[arg(long, env = "RCON_ADDR", default_value = "127.0.0.1:25575")]
    rcon: String,
    /// server.properties: rcon.password
    #[arg(long, env = "RCON_PASSWORD")]
    rcon_password: String,
    /// Target when a method call names no player: the streamer's username, or @a.
    #[arg(long, env = "MC_PLAYER", default_value = "@a")]
    player: String,
    /// The server's logs/latest.log: tailed for chat, joins, advancements and
    /// deaths. Without it the module emits no events.
    #[arg(long, env = "MC_LOG")]
    log: Option<PathBuf>,
}

struct Minecraft {
    addr: String,
    password: String,
    player: String,
    events: bool,
}

impl Handler for Minecraft {
    fn call(&self, method: &str, params: Value) -> MethodFuture {
        let cmds = commands::build(method, &params, &self.player);
        let (addr, password) = (self.addr.clone(), self.password.clone());
        Box::pin(async move {
            // One connection per call: survives server restarts for free.
            let run = async {
                let mut rcon = rcon::connect(&addr, &password).await?;
                let mut output = Vec::new();
                for cmd in cmds? {
                    log::debug!("> {cmd}");
                    output.push(rcon.command(&cmd).await?);
                }
                Ok::<_, anyhow::Error>(output.join("\n"))
            };
            match tokio::time::timeout(Duration::from_secs(10), run).await {
                Ok(Ok(output)) => Ok(json!({ "ok": true, "output": output })),
                Ok(Err(e)) => Err(e
                    .downcast::<RpcError>()
                    .unwrap_or_else(|e| RpcError::internal(format!("rcon: {e}")))),
                Err(_) => Err(RpcError::internal("rcon: timeout")),
            }
        })
    }

    fn subscribe(&self, event: &str) -> MethodFuture<bool> {
        let known = self.events && events::EVENTS.contains(&event);
        Box::pin(async move { known })
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let cli = Cli::parse();

    let mut rcon = rcon::connect(&cli.rcon, &cli.rcon_password)
        .await
        .with_context(|| {
            format!(
                "rcon {}: is enable-rcon=true in server.properties?",
                cli.rcon
            )
        })?;
    log::info!("rcon ok: {}", cli.rcon);
    let mut parser = events::Parser::default();
    parser.seed(&rcon.command("list").await?);
    log::info!("online: {:?}", parser.players);

    std::fs::create_dir_all(&cli.state_dir)?;
    let identity_path = cli.state_dir.join(IDENTITY_FILE);
    let identity: Identity = match cli.enroll {
        Some(token) => {
            let enrolled = enroll(&cli.rawtoh_api, &token).await?;
            write_private(
                &identity_path,
                &serde_json::to_vec_pretty(&enrolled.identity)?,
            )?;
            log::info!(
                "enrolled as {} ({})",
                enrolled.instance_name,
                enrolled.identity.instance_id
            );
            enrolled.identity
        }
        None => serde_json::from_slice(
            &std::fs::read(&identity_path)
                .context("not enrolled: pass --enroll <token> (Module → Instances → Re-enroll)")?,
        )?,
    };

    let hub = HubConnection::new(
        HubOptions {
            url: cli.rawtoh_ws,
            identity,
            label: Some("minecraft".into()),
        },
        Minecraft {
            addr: cli.rcon,
            password: cli.rcon_password,
            player: cli.player,
            events: cli.log.is_some(),
        },
    );
    hub.start().await.context("hub registration")?;

    if let Some(path) = cli.log {
        anyhow::ensure!(path.is_file(), "--log {}: not a file", path.display());
        log::info!("tailing {}", path.display());
        let hub = hub.clone();
        let parser = Mutex::new(parser);
        events::tail(path, move |line| {
            if let Some((event, payload)) = parser.lock().unwrap().parse(line) {
                log::debug!("{event} {payload}");
                hub.emit(event, payload, None);
            }
        });
    }

    let mut status = hub.status();
    tokio::select! {
        _ = status.wait_for(|s| s.reason.is_some()) => anyhow::bail!("hub refused reconnection"),
        _ = tokio::signal::ctrl_c() => {
            hub.stop();
            Ok(())
        }
    }
}

#[cfg(unix)]
fn write_private(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?
        .write_all(bytes)
}

#[cfg(not(unix))]
fn write_private(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::write(path, bytes)
}
