use std::fs;

#[allow(dead_code)]
#[path = "../src/error.rs"]
#[allow(dead_code)]
#[allow(unused)]
mod error;
#[allow(dead_code)]
#[path = "../src/config.rs"]
mod config;

fn write_config(dir: &tempfile::TempDir, secret_path: &str, listen_port: u16) -> String {
    let cfg = format!(
        r#"[threads]
app_id = "app-1"
app_secret_file = "{secret}"
redirect_uri = "http://127.0.0.1:{port}/callback"
user_id = "123"
base_url = "https://graph.threads.net"
version = "v1.0"

[storage]
database_path = "{db}"

[defaults]
link_mode = "reply"
output = "human"
open_browser = true

[oauth]
listen_host = "127.0.0.1"
listen_port = {port}
state_ttl_seconds = 600
"#,
        secret = secret_path,
        db = dir.path().join("threads.db").display(),
        port = listen_port
    );
    let path = dir.path().join("config.toml");
    fs::write(&path, cfg).expect("write config");
    path.display().to_string()
}

#[test]
fn loads_valid_config() {
    let dir = tempfile::tempdir().expect("tempdir");
    let secret = dir.path().join("secret.txt");
    fs::write(&secret, "super-secret").expect("write secret");
    let cfg_path = write_config(&dir, &secret.display().to_string(), 8788);
    let cfg = config::AppConfig::load(Some(&cfg_path)).expect("load config");
    assert_eq!(cfg.oauth.listen_port, 8788);
    assert_eq!(cfg.threads.app_id, "app-1");
}

#[test]
fn rejects_non_localhost_oauth_host() {
    let dir = tempfile::tempdir().expect("tempdir");
    let secret = dir.path().join("secret.txt");
    fs::write(&secret, "super-secret").expect("write secret");
    let cfg = format!(
        r#"[threads]
app_id = "app-1"
app_secret_file = "{secret}"
redirect_uri = "http://127.0.0.1:8788/callback"
user_id = "123"
base_url = "https://graph.threads.net"
version = "v1.0"

[storage]
database_path = "{db}"

[defaults]
link_mode = "reply"
output = "human"
open_browser = true

[oauth]
listen_host = "0.0.0.0"
listen_port = 8788
state_ttl_seconds = 600
"#,
        secret = secret.display(),
        db = dir.path().join("threads.db").display(),
    );
    let path = dir.path().join("config.toml");
    fs::write(&path, cfg).expect("write config");
    let err = config::AppConfig::load(Some(&path.display().to_string())).expect_err("expected fail");
    assert_eq!(err.category.as_code(), "CONFIG_ERROR");
}
