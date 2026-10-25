use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use reqwest::blocking::{multipart, Client};
use serde_json::Value;
use std::process::Command as ProcessCommand;
use std::{fs, path::Path, path::PathBuf};

const DEFAULT_REGISTRY_INDEX: &str =
    "https://raw.githubusercontent.com/nyakokitsu/yapkg/registry/index.json";

#[derive(Parser)]
#[command(name = "byam", about = "BetterYAM package manager")]
struct Cli {
    #[arg(
        long,
        env = "YAM_URL",
        default_value = "http://127.0.0.1:8099",
        global = true
    )]
    url: String,
    #[arg(long, env = "YAM_REGISTRY_URL", global = true)]
    registry: Option<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Install {
        /// Local .yap/.pkg archive, or registry package ID.
        package: PathBuf,
        /// Registry version. When provided, package is treated as a registry ID.
        version: Option<String>,
        /// Print the server response as JSON instead of a confirmation.
        #[arg(long)]
        json: bool,
    },
    /// List installed packages and their status.
    List {
        /// Print raw package data as JSON.
        #[arg(long)]
        json: bool,
    },
    Ps,
    Enable {
        id: String,
    },
    Disable {
        id: String,
    },
    Run {
        id: String,
        #[arg(short, long)]
        detach: bool,
        #[arg(last = true)]
        args: Vec<String>,
    },
    Stop {
        id: String,
    },
    Rm {
        id: String,
    },
    Logs {
        id: String,
    },
    /// Manage background service packages.
    Service {
        #[command(subcommand)]
        command: ServiceCommand,
    },
    /// Open an interactive ADB terminal for an enabled binary oneshot package.
    Tui {
        id: String,
        #[arg(last = true)]
        args: Vec<String>,
    },
    Inspect {
        id: String,
    },
    /// Browse and install packages from a package registry.
    Registry {
        #[command(subcommand)]
        command: RegistryCommand,
    },
    Device,
    Sensors,
    Clock {
        #[command(subcommand)]
        command: ClockCommand,
    },
    Led {
        #[command(subcommand)]
        command: LedCommand,
    },
    /// LED logo alias (`yam logo set` uses the configured LED count).
    Logo {
        #[command(subcommand)]
        command: LedCommand,
    },
}

#[derive(Subcommand)]
enum RegistryCommand {
    /// Browse the registry, optionally filtering by ID, name, or description.
    List {
        query: Option<String>,
        /// Print raw registry entries as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Search registry packages by ID, name, or description.
    Search {
        query: String,
        /// Print raw matching registry entries as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Install a package from the registry.
    #[command(name = "install", alias = "pull")]
    Pull {
        id: String,
        version: Option<String>,
        /// Print the server response as JSON instead of a confirmation.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum ServiceCommand {
    /// Enable a service and start it now.
    Enable { id: String },
    /// Stop a service and prevent it from starting after reboot.
    Disable { id: String },
    /// Start an enabled service.
    Start { id: String },
    /// Stop a service without disabling it.
    Stop { id: String },
    /// Restart an enabled service.
    Restart { id: String },
    /// Show installed service packages and their state.
    List,
    /// Show a service's logs.
    Logs { id: String },
}

#[derive(Subcommand)]
enum LedCommand {
    Set { colors: Vec<String> },
    Off,
}

#[derive(Subcommand)]
enum ClockCommand {
    Status,
    Set {
        time: String,
        #[arg(short, long, default_value_t = 170)]
        brightness: u8,
    },
    Off,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let client = Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(60))
        .build()?;
    match cli.command {
        Command::Install {
            package,
            version,
            json,
        } => {
            let local_archive = package.is_file();
            if local_archive && version.is_some() {
                bail!("a version can only be specified when installing a registry package ID")
            }
            if !local_archive && looks_like_local_archive(&package) {
                bail!("local package archive not found: {}", package.display())
            }
            if !local_archive {
                let id = package
                    .to_str()
                    .context("registry package ID must be valid UTF-8")?;
                install_registry_package(
                    &client,
                    &cli.url,
                    cli.registry.as_deref().unwrap_or(DEFAULT_REGISTRY_INDEX),
                    id,
                    version.as_deref(),
                    json,
                )?;
            } else {
                let file = fs::File::open(&package)
                    .with_context(|| format!("open {}", package.display()))?;
                let part = multipart::Part::reader(file).file_name(
                    package
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string(),
                );
                let response = client
                    .post(endpoint(&cli.url, "/api/plugins"))
                    .multipart(multipart::Form::new().part("package", part))
                    .send()
                    .with_context(|| format!("upload package archive {}", package.display()))?;
                print_install_response(response, json, "local archive")?;
            }
        }
        Command::List { json } => {
            let packages = list_packages(&client, &cli.url)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&packages)?);
            } else {
                print_installed_packages(&packages)?;
            }
        }
        Command::Ps => {
            let packages: Value =
                json_response(client.get(endpoint(&cli.url, "/api/plugins")).send()?)?;
            let empty = Vec::new();
            let running = packages
                .as_array()
                .unwrap_or(&empty)
                .iter()
                .filter(|package| package["running"] == true)
                .collect::<Vec<_>>();
            println!("{}", serde_json::to_string_pretty(&running)?);
        }
        Command::Enable { id } => set_enabled(&client, &cli.url, &id, true)?,
        Command::Disable { id } => set_enabled(&client, &cli.url, &id, false)?,
        Command::Run { id, detach, args } => {
            let log_offset = if detach {
                0
            } else {
                package_logs(&client, &cli.url, &id)?.len()
            };
            print_response(
                client
                    .post(endpoint(&cli.url, &format!("/api/plugins/{id}/invoke")))
                    .json(&serde_json::json!({"args": args}))
                    .send()?,
            )?;
            if !detach {
                wait_for_package(&client, &cli.url, &id)?;
                let log = package_logs(&client, &cli.url, &id)?;
                print!("{}", log.get(log_offset..).unwrap_or(&log));
            }
        }
        Command::Stop { id } => print_response(
            client
                .post(endpoint(&cli.url, &format!("/api/plugins/{id}/stop")))
                .send()?,
        )?,
        Command::Rm { id } => print_response(
            client
                .delete(endpoint(&cli.url, &format!("/api/plugins/{id}")))
                .send()?,
        )?,
        Command::Logs { id } => print_response(
            client
                .get(endpoint(&cli.url, &format!("/api/plugins/{id}/logs")))
                .send()?,
        )?,
        Command::Service { command } => {
            service_command(&client, &cli.url, command)?;
        }
        Command::Tui { id, args } => open_tui(&client, &cli.url, &id, &args)?,
        Command::Inspect { id } => {
            let packages: Value =
                json_response(client.get(endpoint(&cli.url, "/api/plugins")).send()?)?;
            let package = packages
                .as_array()
                .and_then(|items| items.iter().find(|item| item["manifest"]["id"] == id))
                .context("package not found")?;
            println!("{}", serde_json::to_string_pretty(package)?);
        }
        Command::Registry { command } => {
            let registry = cli
                .registry
                .unwrap_or_else(|| DEFAULT_REGISTRY_INDEX.into());
            match command {
                RegistryCommand::List { query, json } => {
                    let packages = registry_packages(&client, &registry)?;
                    print_registry_packages(&packages, query.as_deref(), json)?;
                }
                RegistryCommand::Search { query, json } => {
                    let packages = registry_packages(&client, &registry)?;
                    print_registry_packages(&packages, Some(&query), json)?;
                }
                RegistryCommand::Pull { id, version, json } => {
                    install_registry_package(
                        &client,
                        &cli.url,
                        &registry,
                        &id,
                        version.as_deref(),
                        json,
                    )?;
                }
            }
        }
        Command::Device => print_response(client.get(endpoint(&cli.url, "/api/device")).send()?)?,
        Command::Sensors => print_response(client.get(endpoint(&cli.url, "/api/sensors")).send()?)?,
        Command::Clock { command } => match command {
            ClockCommand::Status => {
                print_response(client.get(endpoint(&cli.url, "/api/clock")).send()?)?
            }
            ClockCommand::Set { time, brightness } => print_response(
                client
                    .put(endpoint(&cli.url, "/api/clock"))
                    .json(&serde_json::json!({"time": time, "brightness": brightness}))
                    .send()?,
            )?,
            ClockCommand::Off => print_response(
                client
                    .put(endpoint(&cli.url, "/api/clock"))
                    .json(&serde_json::json!({"off": true}))
                    .send()?,
            )?,
        },
        Command::Led { command } => match command {
            LedCommand::Set { colors } => print_response(
                client
                    .post(endpoint(&cli.url, "/api/led"))
                    .json(&serde_json::json!({"colors":colors}))
                    .send()?,
            )?,
            LedCommand::Off => {
                print_response(client.post(endpoint(&cli.url, "/api/led/off")).send()?)?
            }
        },
        Command::Logo { command } => match command {
            LedCommand::Set { colors } => print_response(
                client
                    .post(endpoint(&cli.url, "/api/logo"))
                    .json(&serde_json::json!({"colors":colors}))
                    .send()?,
            )?,
            LedCommand::Off => {
                print_response(client.post(endpoint(&cli.url, "/api/logo/off")).send()?)?
            }
        },
    }
    Ok(())
}

fn open_tui(client: &Client, url: &str, id: &str, args: &[String]) -> Result<()> {
    let packages = json_response(client.get(endpoint(url, "/api/plugins")).send()?)?;
    let package = packages
        .as_array()
        .and_then(|items| items.iter().find(|item| item["manifest"]["id"] == id))
        .context("package not found")?;
    if package["manifest"]["launch"]["kind"] != "binary" {
        bail!("TUI is available only for binary packages")
    }
    if package["manifest"]["launch"]["mode"] != "oneshot" {
        bail!("TUI is available only for oneshot packages")
    }
    if package["enabled"] != true {
        bail!("package is disabled")
    }
    let settings = json_response(client.get(endpoint(url, "/api/settings")).send()?)?;
    if settings["allow_plugin_binaries"] != true {
        bail!("binary execution is disabled by administrator")
    }
    let launch = &package["manifest"]["launch"];
    let path = launch["path"]
        .as_str()
        .filter(|path| !path.is_empty())
        .or_else(|| package["manifest"]["entry"].as_str())
        .context("package has no launch path")?;
    let mut command_args = launch["args"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    command_args.extend(args.iter().cloned());
    let serial = adb_station()?;
    // Older daemons do not report the package directory.
    let root = package["directory"]
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| format!("/data/quazar/betteryam/data/plugins/{id}"));
    // Mirrors `launch_program` / `binary_library_path` in BetterYAMd.
    let interpreter = launch["interpreter"].as_str();
    let explicit_libraries = launch["library_paths"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str())
        .map(|directory| format!("{root}/{directory}"))
        .collect::<Vec<_>>();
    let libraries = if explicit_libraries.is_empty() {
        std::path::Path::new(path)
            .parent()
            .and_then(std::path::Path::parent)
            .map(|directory| format!("{root}/{}/lib", directory.display()))
            .into_iter()
            .collect()
    } else {
        explicit_libraries
    };
    let library_setup = if libraries.is_empty() {
        String::new()
    } else {
        format!(
            "export LD_LIBRARY_PATH={}:\"${{LD_LIBRARY_PATH:-}}\" && ",
            shell_quote(&libraries.join(":"))
        )
    };
    let rendered_args = command_args
        .iter()
        .map(|arg| format!(" {}", shell_quote(arg)))
        .collect::<String>();
    let program = shell_quote(&format!("./{path}"));
    let launch_command = match interpreter {
        Some("sh") => format!("{library_setup}exec sh {program}{rendered_args}"),
        Some(other) => bail!("unsupported launch interpreter {other}"),
        None => {
            let legacy_runner = shell_quote(&format!("./{id}/{id}-run"));
            format!(
                "if [ -f {legacy_runner} ]; then exec sh {legacy_runner}{rendered_args}; fi; \
                 {library_setup}exec {program}{rendered_args}"
            )
        }
    };
    let remote = format!("cd {} && {launch_command}", shell_quote(&root));
    let status = ProcessCommand::new("adb")
        .args(["-s", &serial, "shell", "-t", &remote])
        .status()
        .context("start adb terminal")?;
    if !status.success() {
        bail!("TUI exited with {status}")
    }
    Ok(())
}

fn adb_station() -> Result<String> {
    if let Ok(serial) = std::env::var("ADB_SERIAL") {
        if !serial.is_empty() {
            return Ok(serial);
        }
    }
    let output = ProcessCommand::new("adb")
        .arg("devices")
        .output()
        .context("adb is required for byam tui")?;
    let devices = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            match (fields.next(), fields.next()) {
                (Some(serial), Some("device")) => Some(serial.to_owned()),
                _ => None,
            }
        })
        .collect::<Vec<_>>();
    match devices.as_slice() {
        [serial] => Ok(serial.clone()),
        [] => bail!("no ADB device; connect the station with adb first"),
        _ => bail!("multiple ADB devices; set ADB_SERIAL and retry"),
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn looks_like_local_archive(package: &Path) -> bool {
    package.extension().is_some_and(|extension| {
        extension.eq_ignore_ascii_case("yap") || extension.eq_ignore_ascii_case("pkg")
    }) || package.components().count() > 1
}

fn print_installed_packages(packages: &Value) -> Result<()> {
    let packages = packages
        .as_array()
        .context("invalid package list from BetterYAMd")?;
    if packages.is_empty() {
        println!("No packages installed. Browse available packages with `byam registry list`.");
        return Ok(());
    }

    println!(
        "{:<22} {:<24} {:<10} {:<8} {:<8} {:<8} {:<11} NAME",
        "ID", "VERSION", "KIND", "MODE", "ENABLED", "RUNNING", "COMPATIBLE"
    );
    let mut rows = packages.iter().collect::<Vec<_>>();
    rows.sort_by_key(|package| package["manifest"]["id"].as_str().unwrap_or("").to_owned());
    for package in rows {
        let manifest = &package["manifest"];
        let id = table_cell(manifest["id"].as_str().unwrap_or("<unknown>"), 22);
        let version = table_cell(manifest["version"].as_str().unwrap_or("-"), 24);
        let kind = manifest["launch"]["kind"].as_str().unwrap_or("lua");
        let mode = manifest["launch"]["mode"].as_str().unwrap_or("oneshot");
        let enabled = state_label(&package["enabled"]);
        let running = state_label(&package["running"]);
        let compatible = state_label(&package["compatible"]);
        let name = table_cell(manifest["name"].as_str().unwrap_or("<unknown>"), usize::MAX);
        println!(
            "{id:<22} {version:<24} {kind:<10} {mode:<8} {enabled:<8} {running:<8} {compatible:<11} {name}"
        );
    }
    Ok(())
}

fn print_registry_packages(packages: &Value, query: Option<&str>, json: bool) -> Result<()> {
    let packages = packages
        .as_array()
        .context("invalid registry package list")?;
    let query = query.map(str::to_lowercase);
    let mut matching = filter_registry_packages(packages, query.as_deref());
    matching.sort_by(|left, right| {
        left["id"]
            .as_str()
            .unwrap_or("")
            .cmp(right["id"].as_str().unwrap_or(""))
            .then_with(|| {
                left["version"]
                    .as_str()
                    .unwrap_or("")
                    .cmp(right["version"].as_str().unwrap_or(""))
            })
    });

    if json {
        println!("{}", serde_json::to_string_pretty(&matching)?);
        return Ok(());
    }
    if matching.is_empty() {
        match query {
            Some(query) => println!("No registry packages match '{query}'."),
            None => println!("The package registry is empty."),
        }
        return Ok(());
    }

    println!("{:<22} {:<14} {:<24} DESCRIPTION", "ID", "VERSION", "NAME");
    for package in &matching {
        let id = table_cell(package["id"].as_str().unwrap_or("<unknown>"), 22);
        let version = table_cell(package["version"].as_str().unwrap_or("-"), 14);
        let name = table_cell(
            package["name"]
                .as_str()
                .unwrap_or_else(|| package["id"].as_str().unwrap_or("<unknown>")),
            24,
        );
        let description = table_cell(package["description"].as_str().unwrap_or(""), usize::MAX);
        println!("{id:<22} {version:<14} {name:<24} {description}");
    }
    println!("\n{} package version(s)", matching.len());
    Ok(())
}

fn filter_registry_packages<'a>(packages: &'a [Value], query: Option<&str>) -> Vec<&'a Value> {
    let query = query.map(str::to_lowercase);
    packages
        .iter()
        .filter(|package| {
            let Some(query) = query.as_deref() else {
                return true;
            };
            ["id", "name", "description"].iter().any(|field| {
                package[*field]
                    .as_str()
                    .is_some_and(|value| value.to_lowercase().contains(query))
            })
        })
        .collect()
}

fn table_cell(value: &str, max_chars: usize) -> String {
    let sanitized = value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>();
    if sanitized.chars().count() <= max_chars {
        sanitized
    } else if max_chars <= 1 {
        "…".chars().take(max_chars).collect()
    } else {
        format!(
            "{}…",
            sanitized.chars().take(max_chars - 1).collect::<String>()
        )
    }
}

fn install_registry_package(
    client: &Client,
    yam_url: &str,
    registry: &str,
    id: &str,
    version: Option<&str>,
    json: bool,
) -> Result<()> {
    let daemon_response = client
        .post(endpoint(yam_url, "/api/plugins/registry"))
        .json(&serde_json::json!({
            "id": id,
            "version": version,
            "registry": registry,
        }))
        .send()
        .with_context(|| format!("contact BetterYAMd at {yam_url}"))?;
    let status = daemon_response.status();
    let message = daemon_response.text()?;
    if status.is_success() {
        return print_install_response_text(&message, json, "registry");
    }
    if status == reqwest::StatusCode::NOT_FOUND || status == reqwest::StatusCode::METHOD_NOT_ALLOWED
    {
        bail!("BetterYAMd does not support registry downloads; update it with the BetterYAM installer")
    }
    let requested = version.map_or_else(|| id.to_owned(), |version| format!("{id}@{version}"));
    bail!(
        "failed to install {requested} from registry ({status}): {}",
        message.trim()
    )
}

fn print_install_response(
    response: reqwest::blocking::Response,
    json: bool,
    source: &str,
) -> Result<()> {
    let status = response.status();
    let body = response.text()?;
    if !status.is_success() {
        bail!("package installation failed ({status}): {}", body.trim())
    }
    print_install_response_text(&body, json, source)
}

fn print_install_response_text(body: &str, json: bool, source: &str) -> Result<()> {
    if json {
        if !body.trim().is_empty() {
            let value: Value = serde_json::from_str(body)
                .context("BetterYAMd returned an invalid JSON install response")?;
            println!("{}", serde_json::to_string_pretty(&value)?);
        }
    } else if let Ok(manifest) = serde_json::from_str::<Value>(body) {
        let id = manifest["id"].as_str().unwrap_or("<unknown>");
        let name = manifest["name"].as_str().unwrap_or(id);
        let version = manifest["version"].as_str().unwrap_or("");
        println!("Installed {name} ({id} {version}) from {source}.");
    } else if !body.trim().is_empty() {
        println!("{}", body.trim());
    }
    Ok(())
}

fn registry_packages(client: &Client, registry: &str) -> Result<Value> {
    let index_url = registry_index_url(registry);
    let response = client.get(&index_url).send()?;
    if response.status().is_success() {
        let index: Value = response.json()?;
        let packages = index["packages"].clone();
        if !packages.is_array() {
            bail!("invalid static registry index")
        }
        return Ok(packages);
    }
    if response.status() != reqwest::StatusCode::NOT_FOUND {
        return Err(response.error_for_status().unwrap_err().into());
    }
    let response = json_response(client.get(registry_api_url(registry)).send()?)?;
    if let Some(packages) = response.as_array() {
        return Ok(Value::Array(packages.clone()));
    }
    let packages = response["packages"]
        .as_array()
        .context("invalid registry API response: expected a packages array")?;
    Ok(Value::Array(packages.clone()))
}

fn registry_index_url(registry: &str) -> String {
    if registry.trim_end_matches('/').ends_with("index.json") {
        registry.to_owned()
    } else {
        endpoint(registry, "/index.json")
    }
}

fn registry_api_url(registry: &str) -> String {
    let base = registry.trim_end_matches('/');
    let base = base.strip_suffix("/index.json").unwrap_or(base);
    endpoint(base, "/api/packages")
}

fn endpoint(base: &str, path: &str) -> String {
    format!("{}{}", base.trim_end_matches('/'), path)
}

fn json_response(response: reqwest::blocking::Response) -> Result<Value> {
    let response = response.error_for_status()?;
    Ok(response.json()?)
}

fn print_response(response: reqwest::blocking::Response) -> Result<()> {
    let status = response.status();
    let text = response.text()?;
    if !status.is_success() {
        bail!("{}: {}", status, text)
    }
    if !text.trim().is_empty() {
        println!("{text}");
    }
    Ok(())
}

fn set_enabled(client: &Client, url: &str, id: &str, desired: bool) -> Result<()> {
    let packages = json_response(client.get(endpoint(url, "/api/plugins")).send()?)?;
    let enabled = packages
        .as_array()
        .and_then(|items| items.iter().find(|item| item["manifest"]["id"] == id))
        .context("package not found")?["enabled"]
        .as_bool()
        .unwrap_or(false);
    if enabled != desired {
        print_response(
            client
                .post(endpoint(url, &format!("/api/plugins/{id}/toggle")))
                .send()?,
        )?;
    }
    Ok(())
}

fn wait_for_package(client: &Client, url: &str, id: &str) -> Result<()> {
    loop {
        let packages = json_response(client.get(endpoint(url, "/api/plugins")).send()?)?;
        let running = packages
            .as_array()
            .and_then(|items| items.iter().find(|item| item["manifest"]["id"] == id))
            .context("package disappeared")?["running"]
            .as_bool()
            .unwrap_or(false);
        if !running {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

fn package_logs(client: &Client, url: &str, id: &str) -> Result<String> {
    Ok(client
        .get(endpoint(url, &format!("/api/plugins/{id}/logs")))
        .send()?
        .error_for_status()?
        .text()?)
}

fn service_command(client: &Client, url: &str, command: ServiceCommand) -> Result<()> {
    match command {
        ServiceCommand::Enable { id } => {
            let package = get_service(client, url, &id)?;
            if package["enabled"] != true {
                set_enabled(client, url, &id, true)?;
            } else if package["running"] != true {
                start_service(client, url, &id)?;
            }
        }
        ServiceCommand::Disable { id } => {
            let package = get_service(client, url, &id)?;
            if package["running"] == true {
                stop_service(client, url, &id)?;
            }
            if package["enabled"] == true {
                set_enabled(client, url, &id, false)?;
            }
        }
        ServiceCommand::Start { id } => {
            let package = get_service(client, url, &id)?;
            if package["enabled"] != true {
                bail!("service is disabled; run `byam service enable {id}` first")
            }
            if package["running"] != true {
                start_service(client, url, &id)?;
            }
        }
        ServiceCommand::Stop { id } => {
            let package = get_service(client, url, &id)?;
            if package["running"] == true {
                stop_service(client, url, &id)?;
            }
        }
        ServiceCommand::Restart { id } => {
            let package = get_service(client, url, &id)?;
            if package["enabled"] != true {
                bail!("service is disabled; run `byam service enable {id}` first")
            }
            stop_service(client, url, &id)?;
            start_service(client, url, &id)?;
        }
        ServiceCommand::List => {
            let packages = list_packages(client, url)?;
            let services = packages
                .as_array()
                .context("invalid package list from BetterYAMd")?
                .iter()
                .filter(|package| package["manifest"]["launch"]["mode"] == "service")
                .collect::<Vec<_>>();
            println!("{:<24} {:<10} {:<10} NAME", "ID", "ENABLED", "RUNNING");
            for package in services {
                let manifest = &package["manifest"];
                let id = manifest["id"].as_str().unwrap_or("<unknown>");
                let name = manifest["name"].as_str().unwrap_or(id);
                println!(
                    "{id:<24} {:<10} {:<10} {name}",
                    state_label(&package["enabled"]),
                    state_label(&package["running"]),
                );
            }
        }
        ServiceCommand::Logs { id } => {
            get_service(client, url, &id)?;
            print!("{}", package_logs(client, url, &id)?);
        }
    }
    Ok(())
}

fn list_packages(client: &Client, url: &str) -> Result<Value> {
    json_response(client.get(endpoint(url, "/api/plugins")).send()?)
}

fn get_service(client: &Client, url: &str, id: &str) -> Result<Value> {
    let packages = list_packages(client, url)?;
    let package = packages
        .as_array()
        .context("invalid package list from BetterYAMd")?
        .iter()
        .find(|package| package["manifest"]["id"] == id)
        .with_context(|| format!("service package {id} not found"))?;
    if package["manifest"]["launch"]["mode"] != "service" {
        bail!("package {id} is not a service")
    }
    Ok(package.clone())
}

fn start_service(client: &Client, url: &str, id: &str) -> Result<()> {
    print_response(
        client
            .post(endpoint(url, &format!("/api/plugins/{id}/invoke")))
            .send()?,
    )
}

fn stop_service(client: &Client, url: &str, id: &str) -> Result<()> {
    print_response(
        client
            .post(endpoint(url, &format!("/api/plugins/{id}/stop")))
            .send()?,
    )
}

fn state_label(value: &Value) -> &'static str {
    if value == true {
        "yes"
    } else {
        "no"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_distinguishes_registry_ids_from_missing_local_archives() {
        assert!(!looks_like_local_archive(Path::new("nano")));
        assert!(looks_like_local_archive(Path::new("package.yap")));
        assert!(looks_like_local_archive(Path::new("package.PKG")));
        assert!(looks_like_local_archive(Path::new("./packages/nano")));
    }

    #[test]
    fn registry_urls_accept_base_or_index_url() {
        assert_eq!(
            registry_index_url("https://registry.example"),
            "https://registry.example/index.json"
        );
        assert_eq!(
            registry_index_url("https://registry.example/index.json"),
            "https://registry.example/index.json"
        );
        assert_eq!(
            registry_api_url("https://registry.example/index.json"),
            "https://registry.example/api/packages"
        );
    }

    #[test]
    fn registry_search_matches_id_name_and_description_case_insensitively() {
        let packages = vec![
            serde_json::json!({"id":"media-tool","name":"Player","description":"Audio support"}),
            serde_json::json!({"id":"network","name":"Net helper","description":"Utilities"}),
        ];
        assert_eq!(filter_registry_packages(&packages, Some("AUDIO")).len(), 1);
        assert_eq!(
            filter_registry_packages(&packages, Some("PLAYER"))[0]["id"],
            "media-tool"
        );
        assert_eq!(
            filter_registry_packages(&packages, Some("missing")).len(),
            0
        );
        assert_eq!(filter_registry_packages(&packages, None).len(), 2);
    }

    #[test]
    fn table_cells_sanitize_control_characters_and_truncate() {
        assert_eq!(table_cell("hello\nworld", 20), "hello world");
        assert_eq!(table_cell("abcdef", 4), "abc…");
        assert_eq!(table_cell("ééé", 2), "é…");
    }

    #[test]
    fn registry_commands_accept_search_and_global_options_after_subcommand() {
        let cli = Cli::try_parse_from([
            "byam",
            "registry",
            "search",
            "audio",
            "--registry",
            "https://registry.example/index.json",
        ])
        .unwrap();
        assert_eq!(
            cli.registry.as_deref(),
            Some("https://registry.example/index.json")
        );
        assert!(matches!(
            cli.command,
            Command::Registry {
                command: RegistryCommand::Search { query, json: false }
            } if query == "audio"
        ));

        let cli = Cli::try_parse_from(["byam", "registry", "install", "nano", "8.0"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Registry {
                command: RegistryCommand::Pull { id, version: Some(version), json: false }
            } if id == "nano" && version == "8.0"
        ));

        let cli = Cli::try_parse_from(["byam", "registry", "pull", "nano"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Registry {
                command: RegistryCommand::Pull { id, version: None, json: false }
            } if id == "nano"
        ));

        let cli = Cli::try_parse_from(["byam", "list", "--json"]).unwrap();
        assert!(matches!(cli.command, Command::List { json: true }));
    }

    #[test]
    fn service_state_labels_are_human_readable() {
        assert_eq!(state_label(&Value::Bool(true)), "yes");
        assert_eq!(state_label(&Value::Bool(false)), "no");
        assert_eq!(state_label(&Value::Null), "no");
    }
}
