//! `senate-node` — headless Senate: owns the data, runs agents, serves the API and the web UI.
//!
//!   senate-node serve [--bind 127.0.0.1:7777] [--data DIR] [--token T] [--backend claude|codex|mock]
//!   senate-node token            print the access token and URL
//!   senate-node mcp              (internal) MCP bridge launched by agent CLIs

use senate_node::{default_data_dir, serve, Config};

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("serve");
    match cmd {
        "mcp" => senate_node::mcp::run_stdio().await,
        "token" => {
            let dir = arg(&args, "--data").map(Into::into).unwrap_or_else(default_data_dir);
            match std::fs::read_to_string(dir.join("token")) {
                Ok(t) => println!("{}", t.trim()),
                Err(_) => eprintln!("токен ещё не создан — запустите `senate-node serve`"),
            }
        }
        "--version" | "-V" | "version" => println!("senate-node {}", env!("CARGO_PKG_VERSION")),
        "serve" | "" => {
            tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into())).init();
            let mut cfg = Config::default();
            if let Some(b) = arg(&args, "--bind") {
                cfg.bind = b.parse().unwrap_or_else(|_| {
                    eprintln!("неверный адрес --bind: {b}");
                    std::process::exit(2)
                });
            }
            if let Some(d) = arg(&args, "--data") {
                cfg.data_dir = d.into();
            }
            cfg.token = arg(&args, "--token").or_else(|| std::env::var("SENATE_TOKEN").ok());
            cfg.default_backend = arg(&args, "--backend");
            let h = match serve(cfg).await {
                Ok(h) => h,
                Err(e) => {
                    eprintln!("Senate не запустился: {e}");
                    std::process::exit(1)
                }
            };
            println!("Senate node: {}", h.url());
            println!("Токен доступа: {}", h.token);
            println!("Открыть в браузере: {}/#token={}", h.url(), h.token);
            if h.addr.ip().is_unspecified() {
                println!("Сервер доступен по сети. Ограничьте доступ файрволом или держите за HTTPS-прокси.");
            }
            let _ = tokio::signal::ctrl_c().await;
            println!("Останавливаю…");
            h.stop().await;
        }
        other => {
            eprintln!("неизвестная команда: {other}\nкоманды: serve | token | mcp | version");
            std::process::exit(2);
        }
    }
}
