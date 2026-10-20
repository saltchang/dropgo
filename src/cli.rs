//! Client commands: a thin layer over `/api/v1` for coding agents. JSON on stdout, errors as JSON on stderr.

use std::io::{IsTerminal, Read, Write};
use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use reqwest::{Method, RequestBuilder, multipart};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};

#[derive(Subcommand, Debug)]
pub enum Client {
    /// Save an API token from the tokens page, or show who the saved token belongs to.
    Auth {
        #[command(subcommand)]
        cmd: Auth,
    },
    /// Check whether a share path is free.
    Check { path: String },
    /// Upload a file, a folder, a .zip, or `-` for stdin. Prints the new share.
    Upload(UploadArgs),
    /// List active shares, or the trash, newest first.
    List {
        #[arg(long)]
        trash: bool,
    },
    /// Show one share. REF is a share id or path.
    Get { r#ref: String },
    /// Change a share's path, visibility, password or expiry.
    Edit(EditArgs),
    /// Move a share to the trash. It is deleted for good after 30 days.
    Trash { r#ref: String },
    /// Restore a share from the trash, at its old path or at --path.
    Restore {
        r#ref: String,
        #[arg(long)]
        path: Option<String>,
    },
    /// List a share's files, or print one file's bytes.
    Cat {
        r#ref: String,
        file: Option<String>,
        /// Print the rendered HTML of a Markdown file.
        #[arg(long)]
        rendered: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum Auth {
    /// Ask for the URL and the token, check them with the server, then save them.
    Login {
        /// The admin URL, such as https://drop.example.com.
        #[arg(long)]
        url: Option<String>,
    },
    /// Show the URL and the token's name and expiry.
    Status,
}

#[derive(Args, Debug)]
pub struct UploadArgs {
    /// A file, a folder, a .zip, or `-` to read one file from stdin.
    source: String,
    /// File name for stdin, such as notes.md.
    #[arg(long)]
    name: Option<String>,
    /// Share path. Random when left out.
    #[arg(long)]
    path: Option<String>,
    #[arg(long, value_parser = ["public", "password", "owner"])]
    visibility: Option<String>,
    #[arg(long)]
    password: Option<String>,
    /// never, 1d, 7d, 30d, or an RFC 3339 time.
    #[arg(long)]
    expires: Option<String>,
    /// Overwrite the share at --path, keeping the settings not given here.
    #[arg(long)]
    replace: bool,
}

#[derive(Args, Debug)]
pub struct EditArgs {
    r#ref: String,
    #[arg(long)]
    path: Option<String>,
    #[arg(long, value_parser = ["public", "password", "owner"])]
    visibility: Option<String>,
    #[arg(long)]
    password: Option<String>,
    /// never, 1d, 7d, 30d, or an RFC 3339 time.
    #[arg(long)]
    expires: Option<String>,
}

/// A failure to print on stderr, with its exit code.
#[derive(Debug)]
pub struct Failure {
    exit: i32,
    body: Value,
}

fn fail(exit: i32, code: &str, message: impl Into<String>, details: Value) -> Failure {
    Failure { exit, body: json!({ "error": { "code": code, "message": message.into(), "details": details } }) }
}

fn bad_input(message: impl Into<String>) -> Failure {
    fail(6, "bad_input", message, json!({}))
}

fn network(e: impl std::fmt::Display) -> Failure {
    fail(7, "network", e.to_string(), json!({}))
}

/// Runs a client command and returns the exit code.
pub async fn run(cmd: Client) -> i32 {
    match dispatch(cmd).await {
        Ok(()) => 0,
        Err(f) => {
            eprintln!("{}", f.body);
            f.exit
        }
    }
}

fn print(v: &Value) {
    println!("{}", serde_json::to_string_pretty(v).unwrap_or_default());
}

async fn dispatch(cmd: Client) -> Result<(), Failure> {
    let api = match cmd {
        Client::Auth { cmd: Auth::Login { url } } => return login(url).await,
        _ => Api::from_config()?,
    };
    match cmd {
        Client::Auth { .. } => {
            let me = api.json(api.req(Method::GET, "/me")).await?;
            print(&json!({ "url": api.url, "name": me["name"], "expires_at": me["expires_at"] }));
        }
        Client::Check { path } => print(&api.json(api.req(Method::GET, "/check").query(&[("path", path)])).await?),
        Client::List { trash } => {
            let state = if trash { "trash" } else { "active" };
            print(&api.json(api.req(Method::GET, "/shares").query(&[("state", state)])).await?)
        }
        Client::Get { r#ref } => print(&api.share(&r#ref, false).await?),
        Client::Trash { r#ref } => {
            let id = id_of(&api.share(&r#ref, false).await?);
            print(&api.json(api.req(Method::POST, &format!("/shares/{id}/trash"))).await?)
        }
        Client::Restore { r#ref, path } => {
            let id = id_of(&api.share(&r#ref, true).await?);
            let body = path.map_or(json!({}), |p| json!({ "path": p }));
            print(&api.json(api.req(Method::POST, &format!("/shares/{id}/restore")).json(&body)).await?)
        }
        Client::Edit(a) => {
            let id = id_of(&api.share(&a.r#ref, false).await?);
            let mut body = serde_json::Map::new();
            if let Some(p) = a.path {
                body.insert("path".into(), p.into());
            }
            if let Some(v) = a.visibility {
                body.insert("visibility".into(), v.into());
            }
            if let Some(p) = a.password {
                body.insert("password".into(), p.into());
            }
            if let Some(e) = a.expires {
                body.insert("expires_at".into(), expires_value(&e)?);
            }
            print(&api.json(api.req(Method::PATCH, &format!("/shares/{id}")).json(&body)).await?)
        }
        Client::Cat { r#ref, file, rendered } => {
            let id = id_of(&api.share(&r#ref, false).await?);
            match file {
                None => print(&api.json(api.req(Method::GET, &format!("/shares/{id}/files"))).await?),
                Some(f) => {
                    let mut url =
                        reqwest::Url::parse(&format!("{}/api/v1/shares/{id}/files", api.url)).map_err(network)?;
                    url.path_segments_mut().map_err(|_| bad_input("bad URL"))?.extend(f.split('/'));
                    let mut rb = api.http.get(url);
                    if rendered {
                        rb = rb.query(&[("rendered", "1")]);
                    }
                    let bytes = api.send(rb).await?.bytes().await.map_err(network)?;
                    std::io::stdout().write_all(&bytes).map_err(network)?;
                }
            }
        }
        Client::Upload(a) => print(&upload(&api, a).await?),
    }
    Ok(())
}

fn id_of(share: &Value) -> String {
    share["id"].as_str().unwrap_or_default().to_string()
}

/// `never` clears the expiry; the presets become a time; anything else goes to the server as is.
fn expires_value(e: &str) -> Result<Value, Failure> {
    let days = match e {
        "never" => return Ok(Value::Null),
        "1d" => 1,
        "7d" => 7,
        "30d" => 30,
        other => return Ok(other.into()),
    };
    let t = OffsetDateTime::now_utc() + Duration::days(days);
    Ok(t.format(&Rfc3339).map_err(|e| bad_input(e.to_string()))?.into())
}

// ---------- config ----------

#[derive(Serialize, Deserialize, Default)]
struct FileConfig {
    url: Option<String>,
    token: Option<String>,
}

fn config_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config"));
    base.join("dropgo").join("config.toml")
}

fn read_config() -> FileConfig {
    std::fs::read_to_string(config_path()).ok().and_then(|s| toml::from_str(&s).ok()).unwrap_or_default()
}

/// The admin URL without a trailing slash. Plain HTTP only to this machine, so a token never crosses
/// the network in clear text.
fn check_url(raw: &str) -> Result<String, Failure> {
    let url = reqwest::Url::parse(raw.trim()).map_err(|_| bad_input(format!("not a URL: {raw}")))?;
    let local = match url.host() {
        Some(url::Host::Domain(d)) => d == "localhost",
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    };
    match url.scheme() {
        "https" => {}
        "http" if local => {}
        _ => {
            return Err(fail(
                3,
                "config.insecure_url",
                "Use an https:// URL. http:// is only allowed to localhost.",
                json!({ "url": raw }),
            ));
        }
    }
    Ok(url.as_str().trim_end_matches('/').to_string())
}

struct Api {
    url: String,
    token: String,
    http: reqwest::Client,
}

impl Api {
    fn new(url: String, token: String) -> Api {
        Api { url, token, http: reqwest::Client::new() }
    }

    /// Env vars win over the config file. The saved token only goes to the saved URL.
    fn from_config() -> Result<Api, Failure> {
        let file = read_config();
        let env = |k| std::env::var(k).ok().filter(|v| !v.is_empty());
        let url = env("DROPGO_URL").or(file.url.clone()).map(|u| check_url(&u)).transpose()?;
        let saved =
            file.token.filter(|_| url.is_some() && url == file.url.as_deref().map(check_url).and_then(Result::ok));
        match (url, env("DROPGO_TOKEN").or(saved)) {
            (Some(url), Some(token)) => Ok(Api::new(url, token)),
            _ => Err(fail(
                3,
                "config.missing",
                "Not signed in. Run `dropgo auth login`, or set DROPGO_URL and DROPGO_TOKEN.",
                json!({ "config": config_path() }),
            )),
        }
    }

    fn req(&self, method: Method, path: &str) -> RequestBuilder {
        self.http.request(method, format!("{}/api/v1{path}", self.url))
    }

    /// Sends with the token. A server error comes back with its body unchanged.
    async fn send(&self, rb: RequestBuilder) -> Result<reqwest::Response, Failure> {
        let res = rb.bearer_auth(&self.token).send().await.map_err(network)?;
        let status = res.status();
        if status.is_success() {
            return Ok(res);
        }
        let bytes = res.bytes().await.map_err(network)?;
        let Some(body) = serde_json::from_slice::<Value>(&bytes).ok().filter(|b| b["error"]["code"].is_string()) else {
            return Err(fail(7, "network", format!("HTTP {status} from a server that is not dropgo"), json!({})));
        };
        let exit = match status.as_u16() {
            401 | 403 => 3,
            404 => 4,
            409 => 5,
            400..=499 => 6,
            _ => 7,
        };
        Err(Failure { exit, body })
    }

    async fn json(&self, rb: RequestBuilder) -> Result<Value, Failure> {
        self.send(rb).await?.json().await.map_err(network)
    }

    /// Resolves a share id or path. Paths match active shares, or trashed ones for `restore`.
    async fn share(&self, r: &str, trash: bool) -> Result<Value, Failure> {
        if let Ok(id) = uuid::Uuid::parse_str(r) {
            return self.json(self.req(Method::GET, &format!("/shares/{id}"))).await;
        }
        if r.trim().trim_matches('/').is_empty() {
            return Err(bad_input("Give a share id or path."));
        }
        let state = if trash { "trash" } else { "active" };
        let list = self.json(self.req(Method::GET, "/shares").query(&[("state", state), ("path", r)])).await?;
        match list.as_array().map(Vec::as_slice).unwrap_or_default() {
            [] => Err(fail(4, "share.not_found", format!("There is no share at {r}."), json!({}))),
            [one] => Ok(one.clone()),
            many => {
                let candidates: Vec<Value> =
                    many.iter().map(|s| json!({ "id": s["id"], "trashed_at": s["trashed_at"] })).collect();
                Err(fail(
                    5,
                    "ref.ambiguous",
                    format!("Several shares match {r}. Use an id."),
                    json!({ "candidates": candidates }),
                ))
            }
        }
    }
}

// ---------- auth login ----------

async fn login(url: Option<String>) -> Result<(), Failure> {
    let tty = std::io::stdin().is_terminal();
    let url = match url {
        Some(u) => u,
        None if tty => {
            eprint!("dropgo URL (such as https://drop.example.com): ");
            let mut line = String::new();
            std::io::stdin().read_line(&mut line).map_err(network)?;
            line
        }
        None => return Err(bad_input("Pass --url when the token comes from stdin.")),
    };
    let url = check_url(&url)?;
    let token = if tty {
        rpassword::prompt_password("API token (from the tokens page, not shown): ").map_err(network)?
    } else {
        let mut s = String::new();
        std::io::stdin().read_to_string(&mut s).map_err(network)?;
        s
    };
    let token = token.trim().to_string();
    if token.is_empty() {
        return Err(bad_input("No token given."));
    }
    let api = Api::new(url, token);
    let me = api.json(api.req(Method::GET, "/me")).await?;
    let path = config_path();
    let text = toml::to_string(&FileConfig { url: Some(api.url.clone()), token: Some(api.token.clone()) })
        .map_err(|e| bad_input(e.to_string()))?;
    write_private(&path, &text).map_err(|e| bad_input(format!("cannot write {}: {e}", path.display())))?;
    print(&json!({ "url": api.url, "name": me["name"], "expires_at": me["expires_at"], "config": path }));
    Ok(())
}

/// Writes a file only its owner can read.
fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)?;
    // `mode` only applies to a new file.
    f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    f.write_all(text.as_bytes())
}

// ---------- upload ----------

/// shortcut: files are read into memory, bounded by the server's share limit (100 MB by default).
/// Stream them if that limit grows.
fn file_parts(source: &str, name: Option<&str>) -> Result<Vec<(String, String, Vec<u8>)>, Failure> {
    let read = |p: &Path| std::fs::read(p).map_err(|e| bad_input(format!("{}: {e}", p.display())));
    if source == "-" {
        let name = name.ok_or_else(|| bad_input("Pass --name, such as notes.md, when reading stdin."))?;
        let mut buf = Vec::new();
        std::io::stdin().read_to_end(&mut buf).map_err(network)?;
        return Ok(vec![(format!("f:{name}"), name.to_string(), buf)]);
    }
    let path = Path::new(source);
    if path.is_dir() {
        // The server applies the same rules again; this keeps node_modules off the wire.
        let kept = crate::ignore_rules::kept_files(path).map_err(|e| bad_input(e.to_string()))?;
        return kept.into_iter().map(|rel| Ok((format!("f:{rel}"), rel.clone(), read(&path.join(&rel))?))).collect();
    }
    let file_name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let field = if file_name.to_lowercase().ends_with(".zip") { "zip".to_string() } else { format!("f:{file_name}") };
    Ok(vec![(field, file_name, read(path)?)])
}

async fn upload(api: &Api, a: UploadArgs) -> Result<Value, Failure> {
    let mut visibility = a.visibility;
    let mut expires = a.expires;
    let mut overwrite = None;
    if a.replace {
        let path = a.path.as_deref().ok_or_else(|| bad_input("--replace needs --path."))?;
        // Settings not given come from the share being replaced, so a private share never turns public.
        match api.json(api.req(Method::GET, "/check").query(&[("path", path)])).await {
            Ok(_) => {}
            Err(f) if f.body["error"]["code"] == "path.exists" => {
                let e = &f.body["error"]["details"]["existing"];
                overwrite = e["id"].as_str().map(str::to_string);
                visibility = visibility.or_else(|| e["visibility"].as_str().map(str::to_string));
                expires = expires.or_else(|| Some(e["expires_at"].as_str().unwrap_or("never").to_string()));
            }
            Err(f) => return Err(f),
        }
    }
    let visibility = visibility.unwrap_or_else(|| "owner".into());
    if a.password.is_some() && visibility != "password" {
        return Err(bad_input("--password needs --visibility password."));
    }
    let files = file_parts(&a.source, a.name.as_deref())?;
    // Settings first: the server checks them before it reads the files.
    let mut form = multipart::Form::new().text("visibility", visibility);
    if let Some(p) = a.path {
        form = form.text("path", p);
    }
    if let Some(p) = a.password {
        form = form.text("password", p);
    }
    match expires.as_deref().unwrap_or("never") {
        e @ ("never" | "1d" | "7d" | "30d") => form = form.text("expiry", e.to_string()),
        custom => form = form.text("expiry", "custom").text("expires_at", custom.to_string()),
    }
    if let Some(id) = overwrite {
        form = form.text("overwrite", id);
    }
    for (field, name, bytes) in files {
        form = form.part(field, multipart::Part::bytes(bytes).file_name(name));
    }
    api.json(api.req(Method::POST, "/shares").multipart(form)).await
}
