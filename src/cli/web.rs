use crate::api::schema::{Method, Request, WebStartParams};

use super::send_request;

pub(super) fn run_web_command(args: &[String]) -> std::io::Result<i32> {
    let opts = match parse_args(args) {
        Ok(opts) => opts,
        Err(code) => return Ok(code),
    };

    if opts.tls_cert.is_some() || opts.tls_key.is_some() {
        eprintln!(
            "herdr web: TLS is not wired up yet; --tls-cert/--tls-key are ignored and the connection will use plain HTTP"
        );
    }
    if opts.session.is_some() {
        eprintln!("herdr web: --session is not supported yet; targeting the local session");
    }

    let params = WebStartParams {
        bind_addr: Some(opts.bind.clone()),
        token: opts.token.clone(),
        session_ttl_secs: None,
        idle_timeout_secs: None,
        trust_proxy: opts.trust_proxy,
        public_origins: opts.public_origins.clone(),
    };

    let response = match send_request(&Request {
        id: "cli:web:start".into(),
        method: Method::WebStart(params),
    }) {
        Ok(resp) => resp,
        Err(err) => {
            let msg = err.to_string();
            if msg.contains("connect") || msg.contains("No such file") || msg.contains("refused") {
                eprintln!("herdr web: no herdr session running — start herdr first");
            } else {
                eprintln!("herdr web: {msg}");
            }
            return Ok(1);
        }
    };

    if let Some(error) = response.get("error") {
        let message = error
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown error");
        eprintln!("herdr web: {message}");
        return Ok(1);
    }

    let result = &response["result"];
    let fallback_url = format!("http://{}", opts.bind);
    let url = result
        .get("url")
        .and_then(|v| v.as_str())
        .unwrap_or(&fallback_url);

    let response_mode = result
        .get("mode")
        .and_then(|v| v.as_str())
        .unwrap_or("standalone");

    if result.get("type").and_then(|v| v.as_str()) == Some("web_already_running") {
        let running_mode = response_mode;
        let requested_mode = if opts.trust_proxy {
            "trust-proxy"
        } else {
            "standalone"
        };
        println!("herdr web: already running at {url}");
        if running_mode != requested_mode {
            eprintln!(
                "WARNING: server is running in {running_mode} mode but {requested_mode} was requested; \
                 the requested mode/flags were NOT applied. Restart the web server to change modes."
            );
        } else {
            println!("the auth token was printed when the web server first started");
        }
        if !opts.no_open {
            open_browser(url);
        }
        return Ok(0);
    }

    if response_mode == "trust-proxy" {
        println!("herdr web: {url}");
        println!("mode: trusted-gateway");
        eprintln!(
            "WARNING: trust-proxy mode — herdr does NOT authenticate clients; the upstream gateway must."
        );
    } else {
        let token = result
            .get("token")
            .and_then(|v| v.as_str())
            .unwrap_or("(unknown)");
        println!("herdr web: {url}");
        println!("token: {token}");
    }

    if !opts.no_open {
        open_browser(url);
    }

    Ok(0)
}

#[cfg_attr(test, derive(Debug))]
struct WebOpts {
    bind: String,
    token: Option<String>,
    no_open: bool,
    tls_cert: Option<String>,
    tls_key: Option<String>,
    session: Option<String>,
    trust_proxy: bool,
    public_origins: Vec<String>,
}

fn parse_args(args: &[String]) -> Result<WebOpts, i32> {
    let mut bind = "127.0.0.1:7681".to_string();
    let mut token = None;
    let mut no_open = false;
    let mut tls_cert = None;
    let mut tls_key = None;
    let mut session = None;
    let mut trust_proxy = false;
    let mut public_origins = Vec::new();
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--bind" => {
                i += 1;
                bind = arg_value(args, i, "--bind")?;
                i += 1;
            }
            "--token" => {
                i += 1;
                token = Some(arg_value(args, i, "--token")?);
                i += 1;
            }
            "--no-open" => {
                no_open = true;
                i += 1;
            }
            "--trust-proxy" => {
                trust_proxy = true;
                i += 1;
            }
            "--public-origin" => {
                i += 1;
                public_origins.push(arg_value(args, i, "--public-origin")?);
                i += 1;
            }
            "--tls-cert" => {
                i += 1;
                tls_cert = Some(arg_value(args, i, "--tls-cert")?);
                i += 1;
            }
            "--tls-key" => {
                i += 1;
                tls_key = Some(arg_value(args, i, "--tls-key")?);
                i += 1;
            }
            "--session" => {
                i += 1;
                session = Some(arg_value(args, i, "--session")?);
                i += 1;
            }
            "help" | "--help" | "-h" => {
                print_help();
                return Err(0);
            }
            other => {
                eprintln!("herdr web: unknown option: {other}");
                print_help();
                return Err(2);
            }
        }
    }

    Ok(WebOpts {
        bind,
        token,
        no_open,
        tls_cert,
        tls_key,
        session,
        trust_proxy,
        public_origins,
    })
}

fn arg_value(args: &[String], index: usize, flag: &str) -> Result<String, i32> {
    match args.get(index) {
        Some(v) => Ok(v.clone()),
        None => {
            eprintln!("herdr web: missing value for {flag}");
            Err(2)
        }
    }
}

fn print_help() {
    eprintln!("herdr web — start a web server for browser-based terminal access");
    eprintln!();
    eprintln!("usage: herdr web [OPTIONS]");
    eprintln!();
    eprintln!("options:");
    eprintln!("  --bind <ADDR>            bind address [default: 127.0.0.1:7681]");
    eprintln!("  --token <TOKEN>          use specific auth token (default: auto-generate)");
    eprintln!("  --no-open                don't open browser automatically");
    eprintln!("  --trust-proxy            delegate user auth to upstream gateway (no login form)");
    eprintln!("  --public-origin <ORIGIN> accept this exact Origin on /ws upgrade (repeatable)");
    eprintln!("  --tls-cert <PATH>        TLS certificate file");
    eprintln!("  --tls-key <PATH>         TLS private key file");
    eprintln!("  --session <NAME>         target session name");
}

fn open_browser(url: &str) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    #[test]
    fn defaults_when_no_args() {
        let opts = parse_args(&[]).unwrap();
        assert_eq!(opts.bind, "127.0.0.1:7681");
        assert_eq!(opts.token, None);
        assert!(!opts.no_open);
        assert_eq!(opts.tls_cert, None);
        assert_eq!(opts.tls_key, None);
        assert_eq!(opts.session, None);
        assert!(!opts.trust_proxy);
        assert!(opts.public_origins.is_empty());
    }

    #[test]
    fn parses_all_value_flags() {
        let opts = parse_args(&args(&[
            "--bind",
            "0.0.0.0:9000",
            "--token",
            "secret",
            "--tls-cert",
            "cert.pem",
            "--tls-key",
            "key.pem",
            "--session",
            "work",
        ]))
        .unwrap();
        assert_eq!(opts.bind, "0.0.0.0:9000");
        assert_eq!(opts.token.as_deref(), Some("secret"));
        assert_eq!(opts.tls_cert.as_deref(), Some("cert.pem"));
        assert_eq!(opts.tls_key.as_deref(), Some("key.pem"));
        assert_eq!(opts.session.as_deref(), Some("work"));
    }

    #[test]
    fn parses_no_open() {
        let opts = parse_args(&args(&["--no-open"])).unwrap();
        assert!(opts.no_open);
    }

    #[test]
    fn parses_trust_proxy() {
        let opts = parse_args(&args(&["--trust-proxy"])).unwrap();
        assert!(opts.trust_proxy);
    }

    #[test]
    fn parses_public_origin_repeated() {
        let opts = parse_args(&args(&[
            "--public-origin",
            "https://a.example.com",
            "--public-origin",
            "https://b.example.com",
        ]))
        .unwrap();
        assert_eq!(opts.public_origins.len(), 2);
        assert_eq!(opts.public_origins[0], "https://a.example.com");
        assert_eq!(opts.public_origins[1], "https://b.example.com");
    }

    #[test]
    fn missing_value_after_flag_errors() {
        assert_eq!(parse_args(&args(&["--bind"])).unwrap_err(), 2);
        assert_eq!(parse_args(&args(&["--token"])).unwrap_err(), 2);
        assert_eq!(parse_args(&args(&["--public-origin"])).unwrap_err(), 2);
    }

    #[test]
    fn unknown_flag_errors() {
        assert_eq!(parse_args(&args(&["--bogus"])).unwrap_err(), 2);
    }

    #[test]
    fn help_exits_zero() {
        assert_eq!(parse_args(&args(&["--help"])).unwrap_err(), 0);
        assert_eq!(parse_args(&args(&["help"])).unwrap_err(), 0);
        assert_eq!(parse_args(&args(&["-h"])).unwrap_err(), 0);
    }
}
