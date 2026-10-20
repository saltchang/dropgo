use std::net::IpAddr;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(version, about = "Drop a file or folder, get a share link")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Serve the admin and share hosts (the default).
    Serve(Config),
    /// Rebuild the rendered pages of every share that has not been purged.
    Rerender(Config),
    #[command(flatten)]
    Client(crate::cli::Client),
}

impl Cli {
    /// Parses the arguments. A bare `dropgo`, or one with only server flags, still serves.
    pub fn parse_args(mut args: Vec<std::ffi::OsString>) -> Cli {
        let serve = args
            .get(1)
            .and_then(|a| a.to_str())
            .is_none_or(|a| a.starts_with('-') && !matches!(a, "-h" | "--help" | "-V" | "--version"));
        if serve {
            args.insert(1, "serve".into());
        }
        Cli::parse_from(args)
    }
}

#[derive(Args, Debug, Clone)]
pub struct Config {
    #[arg(long, env = "DROPGO_LISTEN", default_value = "0.0.0.0:8080")]
    pub listen: String,
    /// Origin of the admin host, such as `https://drop.example.com`.
    #[arg(long, env = "DROPGO_ADMIN_ORIGIN", value_parser = parse_origin)]
    pub admin_origin: String,
    /// Origin of the share host, such as `https://s.example.com`.
    #[arg(long, env = "DROPGO_SHARE_ORIGIN", value_parser = parse_origin)]
    pub share_origin: String,
    #[arg(long, env = "DROPGO_OIDC_ISSUER")]
    pub oidc_issuer: String,
    #[arg(long, env = "DROPGO_OIDC_CLIENT_ID")]
    pub oidc_client_id: String,
    #[arg(long, env = "DROPGO_OIDC_CLIENT_SECRET", hide_env_values = true)]
    pub oidc_client_secret: String,
    /// Comma-separated values of the owner claim that may sign in.
    #[arg(long, env = "DROPGO_OWNERS", value_delimiter = ',')]
    pub owners: Vec<String>,
    #[arg(long, env = "DROPGO_OWNER_CLAIM", default_value = "preferred_username")]
    pub owner_claim: String,
    /// Key for unlock cookies and access tokens. At least 32 bytes.
    #[arg(long, env = "DROPGO_SECRET", hide_env_values = true, value_parser = parse_secret)]
    pub secret: String,
    #[arg(long, env = "DROPGO_DATABASE_URL", hide_env_values = true)]
    pub database_url: String,
    /// `s3://bucket/prefix`, `file:///path` or `memory://`.
    #[arg(long, env = "DROPGO_STORE_URL")]
    pub store_url: String,
    #[arg(long, env = "DROPGO_MAX_SHARE_BYTES", default_value_t = 100 * 1024 * 1024)]
    pub max_share_bytes: u64,
    /// Where uploads are received before they are stored. Defaults to TMPDIR.
    #[arg(long, env = "DROPGO_TMP_DIR")]
    pub tmp_dir: Option<PathBuf>,
    /// Comma-separated proxy IPs or CIDRs whose `X-Forwarded-For` hops are skipped.
    #[arg(long, env = "DROPGO_TRUSTED_PROXIES", value_delimiter = ',', value_parser = parse_cidr)]
    pub trusted_proxies: Vec<Cidr>,
}

pub const MAX_FILES: usize = 10_000;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cidr {
    addr: IpAddr,
    bits: u8,
}

impl Cidr {
    pub fn contains(&self, ip: IpAddr) -> bool {
        match (self.addr, ip) {
            (IpAddr::V4(a), IpAddr::V4(b)) => prefix_eq(&a.octets(), &b.octets(), self.bits),
            (IpAddr::V6(a), IpAddr::V6(b)) => prefix_eq(&a.octets(), &b.octets(), self.bits),
            _ => false,
        }
    }
}

fn prefix_eq(a: &[u8], b: &[u8], bits: u8) -> bool {
    let full = (bits / 8) as usize;
    let rest = bits % 8;
    a[..full] == b[..full] && (rest == 0 || (a[full] ^ b[full]) >> (8 - rest) == 0)
}

/// `https://host[:port]`, with a trailing `/` dropped so Origin checks and URLs line up.
fn parse_origin(s: &str) -> Result<String, String> {
    let s = s.trim_end_matches('/');
    match s.split_once("://") {
        // Browsers send the Origin header in lower case.
        Some(("http" | "https", host)) if !host.is_empty() && !host.contains('/') => Ok(s.to_ascii_lowercase()),
        _ => Err(format!("expected an origin like https://host, got {s}")),
    }
}

fn parse_secret(s: &str) -> Result<String, String> {
    if s.len() < 32 { Err("must be at least 32 bytes".into()) } else { Ok(s.to_string()) }
}

pub fn parse_cidr(s: &str) -> Result<Cidr, String> {
    let s = s.trim();
    let (ip, bits) = s.split_once('/').unwrap_or((s, ""));
    let addr: IpAddr = ip.parse().map_err(|_| format!("bad IP: {ip}"))?;
    let max = if addr.is_ipv4() { 32 } else { 128 };
    let bits = if bits.is_empty() { max } else { bits.parse().map_err(|_| format!("bad prefix: {s}"))? };
    if bits > max {
        return Err(format!("bad prefix: {s}"));
    }
    Ok(Cidr { addr, bits })
}

impl Config {
    pub fn admin_host(&self) -> &str {
        host_of(&self.admin_origin)
    }
    pub fn share_host(&self) -> &str {
        host_of(&self.share_origin)
    }
    pub fn tmp_dir(&self) -> PathBuf {
        self.tmp_dir.clone().unwrap_or_else(std::env::temp_dir)
    }
}

fn host_of(origin: &str) -> &str {
    origin.split_once("://").map_or(origin, |(_, h)| h).trim_end_matches('/')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cidr() {
        let c = parse_cidr("10.0.0.0/8").unwrap();
        assert!(c.contains("10.1.2.3".parse().unwrap()));
        assert!(!c.contains("11.0.0.1".parse().unwrap()));
        let c = parse_cidr("192.168.1.7").unwrap();
        assert!(c.contains("192.168.1.7".parse().unwrap()));
        assert!(!c.contains("192.168.1.8".parse().unwrap()));
        let c = parse_cidr("fd00::/12").unwrap();
        assert!(c.contains("fd0a::1".parse().unwrap()));
        assert!(parse_cidr("10.0.0.0/33").is_err());
    }

    #[test]
    fn bare_binary_serves() {
        let parse = |args: &[&str]| Cli::parse_args(args.iter().map(Into::into).collect()).command;
        // Required server settings come from flags here, as they would from env in the image.
        let flags = [
            "--admin-origin=https://a.test",
            "--share-origin=https://s.test",
            "--oidc-issuer=x",
            "--oidc-client-id=x",
            "--oidc-client-secret=x",
            "--owners=a",
            "--secret=0123456789abcdef0123456789abcdef",
            "--database-url=x",
            "--store-url=memory://",
        ];
        let mut args = vec!["dropgo"];
        args.extend(flags);
        assert!(matches!(parse(&args), Command::Serve(_)));
        assert!(matches!(parse(&["dropgo", "list"]), Command::Client(crate::cli::Client::List { trash: false })));
    }

    #[test]
    fn origin_and_secret() {
        assert_eq!(parse_origin("https://Drop.example.com/").unwrap(), "https://drop.example.com");
        assert!(parse_origin("drop.example.com").is_err() && parse_origin("https://a.com/x").is_err());
        assert!(parse_secret("short").is_err() && parse_secret(&"x".repeat(32)).is_ok());
    }
}
