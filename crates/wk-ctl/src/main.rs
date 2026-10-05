use std::env;
use std::process::exit;
use wk_ipc::{IpcClient, Request, Response};

fn print_help() {
    println!(
        r#"widgetctl - Command-line control interface for WidgetKit daemon

USAGE:
    widgetctl [--socket <NAME|PATH>] <COMMAND> [OPTIONS]

COMMANDS:
    list                        List every running WidgetKit daemon and its socket
    ping                        Check if the WidgetKit daemon is responding
    status                      Display daemon status, active windows, and services
    toggle <WINDOW_NAME>       Toggle visibility of the specified window (e.g. "topbar", "dock")
    show <WINDOW_NAME>         Show/open the specified window
    hide <WINDOW_NAME>         Hide/destroy the specified window
    reload                     Re-render every registered window
    service <NAME> <ACTION>    Send a command to a registered service
                               Example: widgetctl service audio toggle_mute
                               Example: widgetctl service mpris next

OPTIONS:
    -s, --socket <NAME|PATH>   Target a specific daemon. Accepts a daemon name
                               (as reported by `list`) or a socket path.
                               Defaults to $WIDGETKIT_SOCKET, then the daemon
                               named after the first window.
    -h, --help                 Print this help information

EXAMPLES:
    widgetctl list
    widgetctl --socket topbar status
    widgetctl --socket topbar hide topbar
"#
    );
}

/// Split the global `--socket` / `-s` option out of the argument list.
///
/// Returns the requested target and the remaining positional arguments.
fn take_socket_option(args: &[String]) -> (Option<String>, Vec<String>) {
    let mut target = None;
    let mut rest = Vec::new();
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "-s" | "--socket" => {
                if i + 1 < args.len() {
                    target = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    eprintln!("Error: --socket requires a daemon name or socket path");
                    exit(1);
                }
            }
            other if other.starts_with("--socket=") => {
                target = Some(other["--socket=".len()..].to_string());
                i += 1;
            }
            other => {
                rest.push(other.to_string());
                i += 1;
            }
        }
    }

    (target, rest)
}

fn command_list() -> anyhow::Result<()> {
    let daemons = IpcClient::list_daemons();

    if daemons.is_empty() {
        println!("No WidgetKit daemon found in this session.");
        return Ok(());
    }

    println!("Running WidgetKit daemons:\n");
    for (name, path, reply) in daemons {
        match reply {
            Some(_) => println!("  {:<24} {:<10} {}", name, "online", path.display()),
            None => println!("  {:<24} {:<10} {}", name, "stale", path.display()),
        }
    }

    Ok(())
}

fn main() -> anyhow::Result<()> {
    let argv: Vec<String> = env::args().skip(1).collect();
    let (socket_target, args) = take_socket_option(&argv);

    if args.is_empty() {
        print_help();
        exit(1);
    }

    let cmd = args[0].as_str();

    if matches!(cmd, "list") {
        return command_list();
    }

    let client = IpcClient::resolve(socket_target.as_deref());

    let req = match cmd {
        "ping" => Request::Ping,
        "status" => Request::Status,
        "toggle" => {
            if args.len() < 2 {
                eprintln!("Error: missing window name. Usage: widgetctl toggle <window_name>");
                exit(1);
            }
            Request::ToggleWindow {
                name: args[1].clone(),
            }
        }
        "show" => {
            if args.len() < 2 {
                eprintln!("Error: missing window name. Usage: widgetctl show <window_name>");
                exit(1);
            }
            Request::ShowWindow {
                name: args[1].clone(),
            }
        }
        "hide" => {
            if args.len() < 2 {
                eprintln!("Error: missing window name. Usage: widgetctl hide <window_name>");
                exit(1);
            }
            Request::HideWindow {
                name: args[1].clone(),
            }
        }
        "reload" => Request::Reload,
        "service" => {
            if args.len() < 3 {
                eprintln!("Error: missing service or action. Usage: widgetctl service <service> <action> [payload]");
                exit(1);
            }
            Request::ServiceAction {
                service: args[1].clone(),
                action: args[2].clone(),
                payload: args.get(3).cloned(),
            }
        }
        "-h" | "--help" | "help" => {
            print_help();
            return Ok(());
        }
        unknown => {
            eprintln!("Unknown command: '{}'. Run 'widgetctl --help' for usage.", unknown);
            exit(1);
        }
    };

    match client.send(req) {
        Ok(Response::Pong) => {
            println!("PONG: WidgetKit daemon is active and responding.");
        }
        Ok(Response::Status {
            uptime_secs,
            windows,
            services,
        }) => {
            println!("WidgetKit Daemon Status:");
            println!("  Socket:   {}", client.socket_path().display());
            println!("  Uptime:   {}s", uptime_secs);
            println!("  Windows:  {}", if windows.is_empty() { "none".into() } else { windows.join(", ") });
            println!("  Services: {}", if services.is_empty() { "none".into() } else { services.join(", ") });
        }
        Ok(Response::Ok { message }) => {
            println!("Success: {}", message);
        }
        Ok(Response::Error { message }) => {
            eprintln!("Daemon Error: {}", message);
            exit(1);
        }
        Err(err) => {
            eprintln!("Failed to reach the WidgetKit daemon at {}: {}", client.socket_path().display(), err);
            eprintln!("Is it running? Try `widgetctl list` to see available daemons.");
            exit(1);
        }
    }

    Ok(())
}