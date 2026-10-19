//! agent-cli spec: the real binary against a served test app.
mod common;
use common::*;
use serde_json::Value;
use std::io::Write;
use std::process::{Command, Stdio};

struct Run {
    code: i32,
    stdout: String,
    out: Value,
    err: Value,
}

struct Cli {
    url: String,
    home: tempfile::TempDir,
    app: App,
}

impl Cli {
    async fn new() -> Cli {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let mut cfg = config();
        cfg.admin_origin = url.clone();
        let app = App::with(cfg, dropgo::store::from_url("memory://").unwrap()).await;
        let svc = app.router.clone().into_make_service_with_connect_info::<std::net::SocketAddr>();
        tokio::spawn(async move { axum::serve(listener, svc).await.unwrap() });
        Cli { url, home: tempfile::tempdir().unwrap(), app }
    }

    fn config_file(&self) -> std::path::PathBuf {
        self.home.path().join("dropgo/config.toml")
    }

    async fn run(&self, args: &[&str], stdin: &[u8], env: &[(&str, &str)]) -> Run {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_dropgo"));
        cmd.args(args)
            .env_clear()
            .env("XDG_CONFIG_HOME", self.home.path())
            .envs(env.iter().copied())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let stdin = stdin.to_vec();
        let out = tokio::task::spawn_blocking(move || {
            let mut child = cmd.spawn().unwrap();
            child.stdin.take().unwrap().write_all(&stdin).unwrap();
            child.wait_with_output().unwrap()
        })
        .await
        .unwrap();
        Run {
            code: out.status.code().unwrap(),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            out: serde_json::from_slice(&out.stdout).unwrap_or_default(),
            err: serde_json::from_slice(&out.stderr).unwrap_or_default(),
        }
    }

    /// Logs in through stdin with a fresh token for alice.
    async fn login(&self) {
        let (_, _, t) = self.app.token("alice").await;
        let r = self.run(&["auth", "login", "--url", &self.url], t.as_bytes(), &[]).await;
        assert_eq!(r.code, 0, "{}", r.err);
    }
}

fn code(r: &Run) -> &str {
    r.err["error"]["code"].as_str().unwrap_or_default()
}

#[tokio::test(flavor = "multi_thread")]
async fn login_saves_a_private_config_only_for_a_good_token() {
    use std::os::unix::fs::PermissionsExt;
    let cli = Cli::new().await;
    let r = cli.run(&["auth", "login", "--url", &cli.url], b"dgo_wrong\n", &[]).await;
    assert_eq!((r.code, code(&r)), (3, "auth.invalid_token"));
    assert!(!cli.config_file().exists());

    let (_, _, t) = cli.app.token("alice").await;
    let r = cli.run(&["auth", "login", "--url", &cli.url], format!("{t}\n").as_bytes(), &[]).await;
    assert_eq!(r.code, 0, "{}", r.err);
    assert_eq!(r.out["name"], "test");
    let mode = std::fs::metadata(cli.config_file()).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);

    let r = cli.run(&["auth", "status"], b"", &[]).await;
    assert_eq!((r.code, r.out["url"].as_str()), (0, Some(cli.url.as_str())));
    // The environment wins over the file.
    let r = cli.run(&["auth", "status"], b"", &[("DROPGO_TOKEN", "dgo_wrong")]).await;
    assert_eq!((r.code, code(&r)), (3, "auth.invalid_token"));
    let r = cli.run(&["list"], b"", &[("DROPGO_URL", "http://example.com"), ("DROPGO_TOKEN", "dgo_x")]).await;
    assert_eq!((r.code, code(&r)), (3, "config.insecure_url"));
    // The saved token is never sent to another server.
    let r = cli.run(&["list"], b"", &[("DROPGO_URL", "https://attacker.example")]).await;
    assert_eq!((r.code, code(&r)), (3, "config.missing"));
    let r = cli.run(&["auth", "status"], b"", &[("DROPGO_URL", &format!("{}/", cli.url))]).await;
    assert_eq!(r.code, 0, "{}", r.err);
    // A server that is not dropgo (here the router's unknown-host 404) is a network error.
    let other = cli.url.replace("127.0.0.1", "localhost");
    let r = cli.run(&["get", "notes"], b"", &[("DROPGO_URL", &other), ("DROPGO_TOKEN", &t)]).await;
    assert_eq!((r.code, code(&r)), (7, "network"));
}

#[tokio::test(flavor = "multi_thread")]
async fn not_configured_and_unreachable() {
    let cli = Cli::new().await;
    let r = cli.run(&["list"], b"", &[]).await;
    assert_eq!((r.code, code(&r)), (3, "config.missing"));
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let r = cli
        .run(&["list"], b"", &[("DROPGO_URL", &format!("http://127.0.0.1:{port}")), ("DROPGO_TOKEN", "dgo_x")])
        .await;
    assert_eq!((r.code, code(&r)), (7, "network"));
}

#[tokio::test(flavor = "multi_thread")]
async fn replace_keeps_password_and_expiry() {
    let cli = Cli::new().await;
    cli.login().await;
    let args =
        ["upload", "-", "--name", "index.md", "--path", "pw", "--visibility", "password", "--password", "hunter2"];
    let r = cli.run(&[&args[..], &["--expires", "7d"]].concat(), b"old", &[]).await;
    assert_eq!(r.code, 0, "{}", r.err);
    let expires = r.out["expires_at"].clone();
    let r = cli.run(&["upload", "-", "--name", "index.md", "--path", "pw", "--replace"], b"new", &[]).await;
    assert_eq!(r.code, 0, "{}", r.err);
    assert_eq!((r.out["visibility"].as_str(), &r.out["expires_at"]), (Some("password"), &expires));
    let res = cli.app.nav(&format!("{SHARE}/pw/"), "").await;
    assert!(res.text().contains("current-password") && !res.text().contains("new"), "{}", res.text());
}

#[tokio::test(flavor = "multi_thread")]
async fn upload_replace_and_read() {
    let cli = Cli::new().await;
    cli.login().await;
    let r = cli.run(&["upload", "-", "--name", "index.md", "--path", "notes"], b"# Hi\n", &[]).await;
    assert_eq!(r.code, 0, "{}", r.err);
    let id = r.out["id"].as_str().unwrap().to_string();
    assert_eq!((r.out["visibility"].as_str(), r.out["path"].as_str()), (Some("owner"), Some("notes")));

    let r = cli.run(&["upload", "-", "--name", "a.md", "--password", "x"], b"x", &[]).await;
    assert_eq!(r.code, 6, "{}", r.err);
    let r = cli.run(&["upload", "-", "--name", "a.md", "--path", "notes"], b"x", &[]).await;
    assert_eq!((r.code, code(&r)), (5, "path.exists"));

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("index.md"), "# New\n").unwrap();
    std::fs::create_dir(dir.path().join("node_modules")).unwrap();
    std::fs::write(dir.path().join("node_modules/x.js"), "x").unwrap();
    let r = cli.run(&["upload", dir.path().to_str().unwrap(), "--path", "notes", "--replace"], b"", &[]).await;
    assert_eq!(r.code, 0, "{}", r.err);
    assert_eq!((r.out["id"].as_str(), r.out["file_count"].as_i64()), (Some(id.as_str()), Some(1)));
    assert_eq!(r.out["visibility"], "owner");

    let r = cli.run(&["cat", "notes", "index.md", "--rendered"], b"", &[]).await;
    assert!(r.code == 0 && r.stdout.contains("<h1>New</h1>"), "{}", r.stdout);

    let r = cli.run(&["edit", "notes", "--visibility", "public", "--expires", "7d"], b"", &[]).await;
    assert_eq!((r.code, r.out["visibility"].as_str()), (0, Some("public")));
    assert!(r.out["expires_at"].is_string());
    let r = cli.run(&["get", "nope"], b"", &[]).await;
    assert_eq!((r.code, code(&r)), (4, "share.not_found"));
}

#[tokio::test(flavor = "multi_thread")]
async fn trash_restore_and_ambiguous_ref() {
    let cli = Cli::new().await;
    cli.login().await;
    for _ in 0..2 {
        let r = cli.run(&["upload", "-", "--name", "index.md", "--path", "notes"], b"x", &[]).await;
        assert_eq!(r.code, 0, "{}", r.err);
        let r = cli.run(&["trash", "notes"], b"", &[]).await;
        assert_eq!((r.code, r.out["state"].as_str()), (0, Some("trashed")));
    }
    let r = cli.run(&["list", "--trash"], b"", &[]).await;
    assert_eq!(r.out.as_array().unwrap().len(), 2);
    let r = cli.run(&["restore", "notes"], b"", &[]).await;
    assert_eq!((r.code, code(&r)), (5, "ref.ambiguous"));
    let candidates = r.err["error"]["details"]["candidates"].as_array().unwrap();
    assert_eq!(candidates.len(), 2);
    let id = candidates[0]["id"].as_str().unwrap();
    let r = cli.run(&["restore", id, "--path", "notes-2"], b"", &[]).await;
    assert_eq!((r.code, r.out["path"].as_str()), (0, Some("notes-2")));
}
