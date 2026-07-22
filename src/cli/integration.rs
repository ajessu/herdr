use crate::api::schema::IntegrationTarget;

pub(super) fn run_integration_command(args: &[String]) -> std::io::Result<i32> {
    let Some(subcommand) = args.first().map(|arg| arg.as_str()) else {
        print_integration_help();
        return Ok(2);
    };

    match subcommand {
        "install" => integration_install(&args[1..]),
        "uninstall" => integration_uninstall(&args[1..]),
        "status" => integration_status(&args[1..]),
        "help" | "--help" | "-h" => {
            print_integration_help();
            Ok(0)
        }
        _ => {
            print_integration_help();
            Ok(2)
        }
    }
}

fn integration_status(args: &[String]) -> std::io::Result<i32> {
    let outdated_only = match args {
        [] => false,
        [flag] if flag == "--outdated-only" => true,
        _ => {
            eprintln!("usage: herdr integration status [--outdated-only]");
            return Ok(2);
        }
    };

    if outdated_only {
        crate::integration::print_outdated_update_notice();
        return Ok(0);
    }

    for status in crate::integration::installed_integration_statuses() {
        let target = crate::integration::integration_target_label(status.target);
        let version = match status.installed_version {
            Some(version) => format!("v{version}"),
            None => "legacy".to_string(),
        };
        let state = match status.state {
            crate::integration::IntegrationStatusKind::NotInstalled => "not installed".to_string(),
            crate::integration::IntegrationStatusKind::Current => {
                format!("current ({version})")
            }
            crate::integration::IntegrationStatusKind::Outdated => {
                format!("outdated ({version} < v{})", status.expected_version)
            }
        };
        println!("{target}: {state} ({})", status.path.display());

        // For an installed Claude integration, surface the statusLine wrapper's last
        // confirmed-report epoch so "the model never appears" is distinguishable between
        // wrapper-failing, never-reported, and an unreadable state dir (design FR9b
        // diagnosability). Skipped when not installed — the diagnostic and its PATH note
        // only make sense once the wrapper exists.
        #[cfg(unix)]
        if status.target == IntegrationTarget::Claude
            && status.state != crate::integration::IntegrationStatusKind::NotInstalled
        {
            print_claude_statusline_report();
        }
    }

    Ok(0)
}

/// Print the statusLine wrapper's last-report diagnostic. The rendering and staleness
/// decision live in the pure `crate::integration::format_claude_statusline_report`
/// (unit-tested there); this shell only supplies the current wall-clock time.
#[cfg(unix)]
fn print_claude_statusline_report() {
    // `None` when the clock is unavailable, so the formatter can say "age unknown"
    // rather than the misleading "0s ago" a `unwrap_or(0)` would print.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .ok();
    let report = crate::integration::claude_statusline_report_status();
    for line in crate::integration::format_claude_statusline_report(report, now) {
        println!("{line}");
    }
}

fn integration_install(args: &[String]) -> std::io::Result<i32> {
    let Some(target) = parse_integration_target(args, "install")? else {
        return Ok(2);
    };

    match crate::integration::install_target(target) {
        Ok(messages) => {
            print_integration_messages(messages);
            Ok(0)
        }
        Err(err) => {
            eprintln!("{err}");
            Ok(1)
        }
    }
}

fn integration_uninstall(args: &[String]) -> std::io::Result<i32> {
    let Some(target) = parse_integration_target(args, "uninstall")? else {
        return Ok(2);
    };

    match crate::integration::uninstall_target(target) {
        Ok(messages) => {
            print_integration_messages(messages);
            Ok(0)
        }
        Err(err) => {
            eprintln!("{err}");
            Ok(1)
        }
    }
}

fn print_integration_messages(messages: Vec<String>) {
    for message in messages {
        println!("{message}");
    }
}

fn parse_integration_target(
    args: &[String],
    action: &str,
) -> std::io::Result<Option<IntegrationTarget>> {
    let Some(target) = args.first().map(|arg| arg.as_str()) else {
        eprintln!(
            "usage: herdr integration {action} <pi|omp|claude|codex|copilot|devin|droid|kimi|opencode|kilo|hermes|qodercli|cursor>"
        );
        return Ok(None);
    };
    if args.len() != 1 {
        eprintln!(
            "usage: herdr integration {action} <pi|omp|claude|codex|copilot|devin|droid|kimi|opencode|kilo|hermes|qodercli|cursor>"
        );
        return Ok(None);
    }

    let parsed = match target {
        "pi" => IntegrationTarget::Pi,
        "omp" => IntegrationTarget::Omp,
        "claude" => IntegrationTarget::Claude,
        "codex" => IntegrationTarget::Codex,
        "copilot" => IntegrationTarget::Copilot,
        "devin" => IntegrationTarget::Devin,
        "droid" => IntegrationTarget::Droid,
        "kimi" => IntegrationTarget::Kimi,
        "opencode" => IntegrationTarget::Opencode,
        "kilo" => IntegrationTarget::Kilo,
        "hermes" => IntegrationTarget::Hermes,
        "qodercli" => IntegrationTarget::Qodercli,
        "cursor" => IntegrationTarget::Cursor,
        _ => {
            eprintln!("unknown integration target: {target}");
            eprintln!(
                "currently supported: pi, omp, claude, codex, copilot, devin, droid, kimi, opencode, kilo, hermes, qodercli, cursor"
            );
            return Ok(None);
        }
    };

    Ok(Some(parsed))
}

fn print_integration_help() {
    eprintln!("herdr integration commands:");
    eprintln!("  herdr integration install pi");
    eprintln!("  herdr integration install omp");
    eprintln!("  herdr integration install claude");
    eprintln!("  herdr integration install codex");
    eprintln!("  herdr integration install copilot");
    eprintln!("  herdr integration install devin");
    eprintln!("  herdr integration install droid");
    eprintln!("  herdr integration install kimi");
    eprintln!("  herdr integration install opencode");
    eprintln!("  herdr integration install kilo");
    eprintln!("  herdr integration install hermes");
    eprintln!("  herdr integration install qodercli");
    eprintln!("  herdr integration install cursor");
    eprintln!("  herdr integration uninstall pi");
    eprintln!("  herdr integration uninstall omp");
    eprintln!("  herdr integration uninstall claude");
    eprintln!("  herdr integration uninstall codex");
    eprintln!("  herdr integration uninstall copilot");
    eprintln!("  herdr integration uninstall devin");
    eprintln!("  herdr integration uninstall droid");
    eprintln!("  herdr integration uninstall kimi");
    eprintln!("  herdr integration uninstall opencode");
    eprintln!("  herdr integration uninstall kilo");
    eprintln!("  herdr integration uninstall hermes");
    eprintln!("  herdr integration uninstall qodercli");
    eprintln!("  herdr integration uninstall cursor");
    eprintln!("  herdr integration status [--outdated-only]");
}
