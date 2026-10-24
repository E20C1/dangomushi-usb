use dangomushi::{offline_docs, server, workspace};
use std::env;
use std::path::PathBuf;
use std::process;

const DEFAULT_PORT: u16 = 45_821;
const DEFAULT_WORKSPACE: &str = "workspace";
const DEFAULT_DOC_EXPORT: &str = "dangomushi-offline-docs";

#[derive(Debug)]
struct ServeOptions {
    port: u16,
    workspace: PathBuf,
}

fn main() {
    if let Err(message) = run() {
        eprintln!("dangomushi: {message}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let command = args.next();

    match command.as_deref() {
        None | Some("help") | Some("--help") | Some("-h") => {
            print_help();
            Ok(())
        }
        Some("serve") => {
            let options = parse_serve_options(args.collect())?;
            server::serve(options.port, options.workspace)
                .map_err(|error| format!("server failed: {error}"))
        }
        Some("init") => {
            let root = parse_single_path(args.collect())?;
            workspace::initialize(&root)
                .map_err(|error| format!("could not initialize {}: {error}", root.display()))?;
            println!("initialized workspace: {}", root.display());
            Ok(())
        }
        Some("doctor") => {
            let root = parse_single_path(args.collect())?;
            let report = workspace::doctor(&root);
            println!("DANGOMUSHI DOCTOR");
            println!("workspace: {}", root.display());
            for check in &report.checks {
                let marker = if check.ok { "OK" } else { "MISSING" };
                println!("[{marker:7}] {}", check.label);
            }

            if report.ok() {
                println!("result: OK");
                Ok(())
            } else {
                Err("workspace is incomplete; run dangomushi init".to_string())
            }
        }
        Some("docs") => run_docs(args.collect()),
        Some("version") | Some("--version") | Some("-V") => {
            println!("dangomushi {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some(other) => Err(format!("unknown command: {other}\n\n{}", help_text())),
    }
}

fn run_docs(args: Vec<String>) -> Result<(), String> {
    match args.as_slice() {
        [] | [command] if command == "list" => {
            let summary = offline_docs::summary()
                .map_err(|error| format!("offline docs unavailable: {error}"))?;
            println!("DANGOMUSHI OFFLINE DOCS");
            println!("pack: {}", summary.pack_id);
            println!("format: {}", summary.format_version);
            println!("app_version: {}", summary.app_version);
            println!("documents: {}", summary.document_count);
            println!("bytes: {}", summary.total_bytes);
            println!("sha256: {}", summary.sha256_hex());
            for document in offline_docs::documents() {
                println!("{}", document.path);
            }
            Ok(())
        }
        [command, path] if command == "show" => {
            let document = offline_docs::document(path)
                .map_err(|error| format!("could not open offline document {path}: {error}"))?;
            print!("{}", document.content);
            Ok(())
        }
        [command] if command == "export" => {
            export_docs(PathBuf::from(DEFAULT_DOC_EXPORT))
        }
        [command, destination] if command == "export" => {
            export_docs(PathBuf::from(destination))
        }
        [command] if command == "verify" => {
            verify_docs(PathBuf::from(DEFAULT_DOC_EXPORT))
        }
        [command, destination] if command == "verify" => {
            verify_docs(PathBuf::from(destination))
        }
        _ => Err(docs_help_text()),
    }
}

fn export_docs(destination: PathBuf) -> Result<(), String> {
    let summary = offline_docs::export(&destination)
        .map_err(|error| format!("offline docs export failed: {error}"))?;
    println!("exported offline docs: {}", destination.display());
    println!("documents: {}", summary.document_count);
    println!("sha256: {}", summary.sha256_hex());
    Ok(())
}

fn verify_docs(destination: PathBuf) -> Result<(), String> {
    let summary = offline_docs::verify_export(&destination)
        .map_err(|error| format!("offline docs verification failed: {error}"))?;
    println!("offline docs verified: {}", destination.display());
    println!("documents: {}", summary.document_count);
    println!("sha256: {}", summary.sha256_hex());
    Ok(())
}

fn docs_help_text() -> String {
    format!(
        "Usage:\n  dangomushi docs list\n  dangomushi docs show <PACK-PATH>\n  dangomushi docs export [DESTINATION]\n  dangomushi docs verify [DESTINATION]\n\nDefault destination: ./{DEFAULT_DOC_EXPORT}"
    )
}

fn parse_single_path(args: Vec<String>) -> Result<PathBuf, String> {
    match args.as_slice() {
        [] => Ok(PathBuf::from(DEFAULT_WORKSPACE)),
        [path] => Ok(PathBuf::from(path)),
        _ => Err("expected at most one workspace path".to_string()),
    }
}

fn parse_serve_options(args: Vec<String>) -> Result<ServeOptions, String> {
    let mut options = ServeOptions {
        port: DEFAULT_PORT,
        workspace: PathBuf::from(DEFAULT_WORKSPACE),
    };

    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--port" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| "--port requires a value".to_string())?;
                options.port = value
                    .parse::<u16>()
                    .map_err(|_| format!("invalid port: {value}"))?;
            }
            "--workspace" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| "--workspace requires a path".to_string())?;
                options.workspace = PathBuf::from(value);
            }
            "--help" | "-h" => return Err(help_text()),
            other => return Err(format!("unknown serve option: {other}")),
        }
        index += 1;
    }

    Ok(options)
}

fn print_help() {
    println!("{}", help_text());
}

fn help_text() -> String {
    format!(
        "DANGOMUSHI {}\n\n\
Usage:\n  dangomushi serve [--port PORT] [--workspace PATH]\n  \
dangomushi init [PATH]\n  \
dangomushi doctor [PATH]\n  \
dangomushi docs list|show|export|verify ...\n  \
dangomushi version\n\n\
Desktop:\n  run the dangomushi-desktop binary built with --features desktop\n\n\
Defaults:\n  port: {DEFAULT_PORT}\n  workspace: ./{DEFAULT_WORKSPACE}\n\n\
Security:\n  the web gateway binds to 127.0.0.1 only. Direct peer LAN is a separate library path gated by explicit run-local LAN intent, live trusted-network checks and trusted-peer cryptography; this serve command never exposes it.",
        env!("CARGO_PKG_VERSION")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serve_defaults_are_stable() {
        let options = parse_serve_options(vec![]).expect("parse defaults");
        assert_eq!(options.port, DEFAULT_PORT);
        assert_eq!(options.workspace, PathBuf::from(DEFAULT_WORKSPACE));
    }

    #[test]
    fn serve_options_parse() {
        let options = parse_serve_options(vec![
            "--port".into(),
            "49152".into(),
            "--workspace".into(),
            "usb-workspace".into(),
        ])
        .expect("parse options");

        assert_eq!(options.port, 49_152);
        assert_eq!(options.workspace, PathBuf::from("usb-workspace"));
    }
}
