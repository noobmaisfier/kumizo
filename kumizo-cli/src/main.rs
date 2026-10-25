use std::env;
use std::process::exit;
use kumizo_kit::ipc::{IpcClient, Request, Response};

fn print_help() {
    println!(
        r#"kumizo - Command-line control of the running Kumizo shell

USAGE:
    kumizo <COMMAND> [OPTIONS]

COMMANDS:
    ping                        Check if the shell is responding
    status                      Display uptime, windows, and services
    toggle <WINDOW_NAME>        Toggle visibility of the specified window (e.g. "dock")
    show <WINDOW_NAME>          Show/open the specified window
    hide <WINDOW_NAME>          Hide/destroy the specified window
    reload                      Re-render every registered window
    service <NAME> <ACTION>     Send a command to a registered service
                                Example: kumizo service audio toggle_mute
                                Example: kumizo service mpris next

OPTIONS:
    -h, --help                  Print this help information

Kumizo is single-instance: there is exactly one socket,
$XDG_RUNTIME_DIR/kumizo.sock (override with $KUMIZO_SOCKET).

EXAMPLES:
    kumizo status
    kumizo toggle dock
"#
    );
}

/// Fetch a mandatory positional argument or exit with a usage message.
fn required(args: &[String], index: usize, usage: &str) -> String {
    match args.get(index) {
        Some(value) => value.clone(),
        None => {
            eprintln!("Error: missing argument. Usage: kumizo {usage}");
            exit(1);
        }
    }
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();

    let Some(cmd) = args.first().map(String::as_str) else {
        print_help();
        exit(1);
    };

    let req = match cmd {
        "ping" => Request::Ping,
        "status" => Request::Status,
        "toggle" => Request::ToggleWindow {
            name: required(&args, 1, "toggle <window_name>"),
        },
        "show" => Request::ShowWindow {
            name: required(&args, 1, "show <window_name>"),
        },
        "hide" => Request::HideWindow {
            name: required(&args, 1, "hide <window_name>"),
        },
        "reload" => Request::Reload,
        "service" => Request::ServiceAction {
            service: required(&args, 1, "service <service> <action> [payload]"),
            action: required(&args, 2, "service <service> <action> [payload]"),
            payload: args.get(3).cloned(),
        },
        "-h" | "--help" | "help" => {
            print_help();
            return Ok(());
        }
        unknown => {
            eprintln!("Unknown command: '{unknown}'. Run 'kumizo --help' for usage.");
            exit(1);
        }
    };

    let client = IpcClient::connect();

    match client.send(req) {
        Ok(Response::Pong) => {
            println!("PONG: Kumizo is active and responding.");
        }
        Ok(Response::Status {
            uptime_secs,
            windows,
            services,
        }) => {
            println!("Kumizo Status:");
            println!("  Socket:   {}", client.socket_path().display());
            println!("  Uptime:   {}s", uptime_secs);
            println!("  Windows:  {}", if windows.is_empty() { "none".into() } else { windows.join(", ") });
            println!("  Services: {}", if services.is_empty() { "none".into() } else { services.join(", ") });
        }
        Ok(Response::Ok { message }) => {
            println!("Success: {}", message);
        }
        Ok(Response::Error { message }) => {
            eprintln!("Error: {}", message);
            exit(1);
        }
        Err(err) => {
            eprintln!(
                "Failed to reach Kumizo at {}: {}",
                client.socket_path().display(),
                err
            );
            eprintln!("Is the shell running?");
            exit(1);
        }
    }

    Ok(())
}