use std::path::{Path, PathBuf};

use tracing::warn;

use super::snapshot::{
    parse_history_snapshot, parse_snapshot, snapshot_file_version, SessionHistorySnapshot,
    SessionSnapshot, SNAPSHOT_VERSION,
};

fn session_path() -> PathBuf {
    crate::session::data_dir().join("session.json")
}

fn session_history_path() -> PathBuf {
    crate::session::data_dir().join("session-history.json")
}

// Follow symlinks manually so a write through a (possibly dangling) symlink
// lands on the target. `fs::canonicalize` requires the target to exist, which
// excludes the dangling-symlink case stow users hit on the very first save.
fn resolve_write_target(path: &Path) -> std::io::Result<PathBuf> {
    let mut current = path.to_path_buf();
    for _ in 0..16 {
        let meta = match std::fs::symlink_metadata(&current) {
            Ok(meta) => meta,
            Err(_) => return Ok(current),
        };
        if !meta.file_type().is_symlink() {
            return Ok(current);
        }
        let link = std::fs::read_link(&current)?;
        current = if link.is_absolute() {
            link
        } else {
            current
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(link)
        };
    }
    Ok(current)
}

/// If the file the writer would overwrite has a snapshot version newer than the
/// running binary's SNAPSHOT_VERSION, rename it to a non-colliding `.bak` so we
/// never silently overwrite a newer session on downgrade. Operates on the
/// resolved write target so symlinked session files are protected too.
fn backup_if_newer(path: &Path) -> std::io::Result<()> {
    let target = resolve_write_target(path)?;
    let content = match std::fs::read_to_string(&target) {
        Ok(c) => c,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };
    let on_disk_version = match snapshot_file_version(&content) {
        Some(v) => v,
        None => return Ok(()),
    };
    if on_disk_version <= SNAPSHOT_VERSION {
        return Ok(());
    }
    let Some(bak) = non_colliding_backup_path(&target, on_disk_version) else {
        warn!(
            path = %target.display(),
            on_disk_version,
            "anti-clobber: no free backup slot, refusing to overwrite newer file"
        );
        return Err(std::io::Error::other(
            "anti-clobber: no free backup slot for newer-versioned session file",
        ));
    };
    warn!(
        path = %target.display(),
        on_disk_version,
        supported = SNAPSHOT_VERSION,
        backup = %bak.display(),
        "anti-clobber: backing up newer-versioned file before overwrite"
    );
    std::fs::rename(&target, &bak)
}

/// The single source of truth for the backup filename convention shared by the
/// writer (`backup_if_newer`) and the reader (`recover_backup_to_primary`):
/// `<stem>.v<version>.<n>.bak`.
fn backup_file_name(stem: &str, version: u32, n: u32) -> String {
    format!("{stem}.v{version}.{n}.bak")
}

/// Parse the `version` out of a `<stem>.v<version>.<n>.bak` name, returning
/// `None` unless the name matches the strict convention for `stem`. This keeps
/// recovery from trusting arbitrary `<stem>.v*...bak` files (e.g. a user's
/// manual `session.json.vkeep.bak`) that happen to share the prefix/suffix.
fn backup_version_from_name(stem: &str, name: &str) -> Option<u32> {
    let rest = name.strip_prefix(stem)?.strip_prefix(".v")?;
    let rest = rest.strip_suffix(".bak")?;
    let (version, counter) = rest.split_once('.')?;
    if counter.is_empty() || !counter.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    version.parse().ok()
}

/// Lowest-numbered `<stem>.v<version>.<n>.bak` path that does not yet exist, or
/// `None` if every slot up to the bound is taken (caller must refuse to
/// overwrite rather than clobber an existing backup).
fn non_colliding_backup_path(path: &Path, version: u32) -> Option<PathBuf> {
    let stem = path.file_name()?.to_string_lossy().to_string();
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    (0..1000)
        .map(|n| parent.join(backup_file_name(&stem, version, n)))
        .find(|candidate| !candidate.exists())
}

/// Single-path session save used by tests. Production saves session and history
/// together via `save_to_paths` so both anti-clobber guards run atomically.
#[cfg(test)]
pub(super) fn save_to_path(path: &Path, snapshot: &SessionSnapshot) -> std::io::Result<()> {
    backup_if_newer(path)?;
    save_json_to_path(path, snapshot)
}

fn save_json_to_path<T: serde::Serialize>(path: &Path, snapshot: &T) -> std::io::Result<()> {
    let target = resolve_write_target(path)?;
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(snapshot)?;
    let tmp_path = target.with_extension("json.tmp");
    std::fs::write(&tmp_path, &json)?;
    if let Err(err) = std::fs::rename(&tmp_path, &target) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(err);
    }
    Ok(())
}

pub(super) fn save_to_paths(
    session_path: &Path,
    history_path: &Path,
    snapshot: &SessionSnapshot,
    history: Option<&SessionHistorySnapshot>,
) -> std::io::Result<()> {
    // The session and history files are versioned together. Run both
    // anti-clobber guards up front so a failure on either leaves *both* files
    // untouched — never the torn state of a downgraded session beside an
    // un-guarded history file.
    backup_if_newer(session_path)?;
    backup_if_newer(history_path)?;
    save_json_to_path(session_path, snapshot)?;
    if let Some(history) = history {
        save_json_to_path(history_path, history)?;
    } else {
        // The newer history file (if any) was already backed up above; just
        // remove whatever current-or-older file remains.
        remove_file_if_present(history_path)?;
    }
    Ok(())
}

pub(super) fn clear_path(path: &Path) -> std::io::Result<()> {
    backup_if_newer(path)?;
    remove_file_if_present(path)
}

fn remove_file_if_present(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

pub fn save(snapshot: &SessionSnapshot, history: Option<&SessionHistorySnapshot>) {
    let path = session_path();
    let history_path = session_history_path();
    if let Err(err) = save_to_paths(&path, &history_path, snapshot, history) {
        crate::logging::session_save_failed(&path, &err.to_string());
        return;
    }
    crate::logging::session_saved(&path, snapshot.workspaces.len());
}

pub fn clear() {
    let path = session_path();
    if let Err(err) = clear_path(&path) {
        crate::logging::session_clear_failed(&path, &err.to_string());
        return;
    }
    clear_history();
    crate::logging::session_cleared(&path);
}

pub fn clear_history() {
    let path = session_history_path();
    if let Err(err) = clear_path(&path) {
        crate::logging::session_clear_failed(&path, &err.to_string());
    }
}

pub fn load() -> Option<SessionSnapshot> {
    let path = session_path();
    if let Some(content) = recover_backup_to_primary(&path, |c| parse_snapshot(c).is_ok()) {
        if let Ok(snapshot) = parse_snapshot(&content) {
            return Some(snapshot);
        }
    }
    if !path.exists() {
        return None;
    }
    let content = match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(err) => {
            warn!(err = %err, "failed to read session file");
            return None;
        }
    };
    match parse_snapshot(&content) {
        Ok(snapshot) => Some(snapshot),
        Err(err) => {
            if let Some(version) = snapshot_file_version(&content) {
                if version > SNAPSHOT_VERSION {
                    warn!(
                        file_version = version,
                        supported = SNAPSHOT_VERSION,
                        "session file is from a newer herdr version, ignoring"
                    );
                    return None;
                }
            }
            warn!(err = %err, "failed to parse session file, ignoring");
            None
        }
    }
}

/// If a strictly-named `<primary>.v<N>.<n>.bak` whose content version matches
/// `N` is now readable by this binary, and the primary is missing, unreadable,
/// or older, restore the highest such backup to the primary path and return its
/// content. `readable` validates that this binary can parse a candidate.
/// Superseded backups (now-readable but not the winner) are removed; backups
/// still newer than this binary are left in place.
fn recover_backup_to_primary(path: &Path, readable: impl Fn(&str) -> bool) -> Option<String> {
    let parent = path.parent()?;
    let stem = path.file_name()?.to_string_lossy().to_string();
    let mut readable_backups: Vec<(u32, PathBuf, String)> = Vec::new();
    for entry in std::fs::read_dir(parent).ok()?.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        // Only trust files matching the exact `<stem>.v<N>.<n>.bak` convention
        // herdr writes — never an arbitrary same-prefix file in data_dir().
        let Some(name_version) = backup_version_from_name(&stem, &name_str) else {
            continue;
        };
        if name_version > SNAPSHOT_VERSION {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        // Require the content to be self-consistent with its filename version,
        // so an empty/unrelated `{}` (which parses as version 0) can't be
        // promoted by sitting in a `.v0.0.bak` name.
        if snapshot_file_version(&content) != Some(name_version) || !readable(&content) {
            continue;
        }
        readable_backups.push((name_version, entry.path(), content));
    }
    let (version, bak_path, content) = readable_backups
        .iter()
        .max_by_key(|(version, _, _)| *version)
        .map(|(v, p, c)| (*v, p.clone(), c.clone()))?;

    // Only step aside for a primary this binary can actually read and that is
    // at least as new as the backup. A corrupt or unreadable primary must not
    // shadow a perfectly recoverable backup just because its version field
    // happens to parse.
    let primary = std::fs::read_to_string(path).ok();
    let primary_usable = primary.as_deref().is_some_and(&readable);
    let primary_version = primary.as_deref().and_then(snapshot_file_version);
    if primary_usable && primary_version.is_some_and(|pv| pv >= version) {
        return None;
    }
    warn!(
        backup = %bak_path.display(),
        version,
        "anti-clobber recovery: restoring from backup"
    );
    if let Err(err) = std::fs::rename(&bak_path, path) {
        warn!(err = %err, "anti-clobber recovery: failed to restore backup to primary path");
        return None;
    }
    // GC any other now-readable backups the recovery superseded.
    for (_, other, _) in &readable_backups {
        if other != &bak_path {
            let _ = std::fs::remove_file(other);
        }
    }
    Some(content)
}

pub fn load_history() -> Option<SessionHistorySnapshot> {
    let path = session_history_path();
    if let Some(content) = recover_backup_to_primary(&path, |c| parse_history_snapshot(c).is_ok()) {
        if let Ok(snapshot) = parse_history_snapshot(&content) {
            return Some(snapshot);
        }
    }
    if !path.exists() {
        return None;
    }
    let content = match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(err) => {
            warn!(err = %err, "failed to read session history file");
            return None;
        }
    };
    match parse_history_snapshot(&content) {
        Ok(snapshot) => Some(snapshot),
        Err(err) => {
            if let Some(version) = snapshot_file_version(&content) {
                if version > SNAPSHOT_VERSION {
                    warn!(
                        file_version = version,
                        supported = SNAPSHOT_VERSION,
                        "session history file is from a newer herdr version, ignoring"
                    );
                    return None;
                }
            }
            warn!(err = %err, "failed to parse session history file, ignoring");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persist::snapshot::{
        PaneHistorySnapshot, TabHistorySnapshot, WorkspaceHistorySnapshot,
    };

    fn temp_session_path(name: &str) -> PathBuf {
        let unique = format!(
            "herdr-session-tests-{}-{}-{}",
            name,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        std::env::temp_dir().join(unique).join("session.json")
    }

    fn temp_session_paths(name: &str) -> (PathBuf, PathBuf) {
        let session = temp_session_path(name);
        let history = session.with_file_name("session-history.json");
        (session, history)
    }

    fn empty_snapshot() -> SessionSnapshot {
        SessionSnapshot {
            version: SNAPSHOT_VERSION,
            workspaces: vec![],
            active: None,
            selected: 0,
            sidebar_width: Some(26),
            sidebar_section_split: Some(0.5),
            collapsed_space_keys: std::collections::HashSet::new(),
        }
    }

    fn history_snapshot(secret: &str) -> SessionHistorySnapshot {
        SessionHistorySnapshot {
            version: SNAPSHOT_VERSION,
            workspaces: vec![WorkspaceHistorySnapshot {
                tabs: vec![TabHistorySnapshot {
                    panes: std::collections::HashMap::from([(
                        0,
                        PaneHistorySnapshot {
                            ansi: secret.to_string(),
                            lines: 1,
                        },
                    )]),
                }],
            }],
        }
    }

    #[test]
    fn save_to_paths_writes_pane_history_only_to_history_file() {
        let (session_path, history_path) = temp_session_paths("split-history");

        save_to_paths(
            &session_path,
            &history_path,
            &empty_snapshot(),
            Some(&history_snapshot("split-secret")),
        )
        .unwrap();

        let session = std::fs::read_to_string(&session_path).unwrap();
        let history = std::fs::read_to_string(&history_path).unwrap();
        assert!(!session.contains("split-secret"));
        assert!(!session.contains("history"));
        assert!(history.contains("split-secret"));
    }

    #[test]
    fn save_to_paths_removes_stale_history_when_history_is_disabled() {
        let (session_path, history_path) = temp_session_paths("clear-history");
        save_to_paths(
            &session_path,
            &history_path,
            &empty_snapshot(),
            Some(&history_snapshot("stale-secret")),
        )
        .unwrap();

        save_to_paths(&session_path, &history_path, &empty_snapshot(), None).unwrap();

        assert!(session_path.exists());
        assert!(!history_path.exists());
    }

    #[test]
    fn clear_path_removes_existing_session_file() {
        let path = temp_session_path("clear-existing");
        save_to_path(&path, &empty_snapshot()).unwrap();

        clear_path(&path).unwrap();

        assert!(!path.exists());
    }

    #[test]
    fn clear_path_ignores_missing_session_file() {
        let path = temp_session_path("clear-missing");

        clear_path(&path).unwrap();

        assert!(!path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn save_to_path_preserves_existing_symlink() {
        let target = temp_session_path("symlink-target");
        let link = target.with_file_name("link.json");
        save_to_path(&target, &empty_snapshot()).unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let mut snap = empty_snapshot();
        snap.selected = 7;
        save_to_path(&link, &snap).unwrap();

        assert!(std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        let parsed = parse_snapshot(&std::fs::read_to_string(&target).unwrap()).unwrap();
        assert_eq!(parsed.selected, 7);
    }

    #[cfg(unix)]
    #[test]
    fn save_to_path_writes_through_dangling_symlink() {
        let target = temp_session_path("dangling-target");
        let link = target.with_file_name("link.json");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        save_to_path(&link, &empty_snapshot()).unwrap();

        assert!(std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert!(target.exists());
    }

    #[cfg(unix)]
    #[test]
    fn save_to_path_resolves_relative_symlink() {
        let session = temp_session_path("relative-symlink");
        let dir = session.parent().unwrap();
        std::fs::create_dir_all(dir).unwrap();
        let target = dir.join("real.json");
        let link = dir.join("link.json");
        std::os::unix::fs::symlink("real.json", &link).unwrap();

        save_to_path(&link, &empty_snapshot()).unwrap();

        assert!(std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert!(target.exists());
    }

    fn newer_version_json(version: u32) -> String {
        serde_json::json!({
            "version": version,
            "workspaces": [],
            "active": null,
            "selected": 0
        })
        .to_string()
    }

    #[test]
    fn anti_clobber_backs_up_newer_versioned_file() {
        let path = temp_session_path("anti-clobber-save");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, newer_version_json(SNAPSHOT_VERSION + 1)).unwrap();

        save_to_path(&path, &empty_snapshot()).unwrap();

        assert!(path.exists());
        let parsed = parse_snapshot(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(parsed.version, SNAPSHOT_VERSION);
        let bak = path.with_file_name(format!("session.json.v{}.0.bak", SNAPSHOT_VERSION + 1));
        assert!(bak.exists());
        let bak_version = snapshot_file_version(&std::fs::read_to_string(&bak).unwrap());
        assert_eq!(bak_version, Some(SNAPSHOT_VERSION + 1));
    }

    #[test]
    fn anti_clobber_non_colliding_backup_names() {
        let path = temp_session_path("anti-clobber-non-collide");
        let parent = path.parent().unwrap();
        std::fs::create_dir_all(parent).unwrap();
        let bak0 = parent.join(format!("session.json.v{}.0.bak", SNAPSHOT_VERSION + 1));
        std::fs::write(&bak0, "existing backup").unwrap();
        std::fs::write(&path, newer_version_json(SNAPSHOT_VERSION + 1)).unwrap();

        save_to_path(&path, &empty_snapshot()).unwrap();

        assert!(bak0.exists());
        assert_eq!(std::fs::read_to_string(&bak0).unwrap(), "existing backup");
        let bak1 = parent.join(format!("session.json.v{}.1.bak", SNAPSHOT_VERSION + 1));
        assert!(bak1.exists());
    }

    #[test]
    fn anti_clobber_does_not_fire_for_current_or_older_version() {
        let path = temp_session_path("anti-clobber-older");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, newer_version_json(SNAPSHOT_VERSION)).unwrap();

        save_to_path(&path, &empty_snapshot()).unwrap();

        let entries: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".bak"))
            .collect();
        assert!(entries.is_empty());
    }

    #[test]
    fn clear_path_backs_up_newer_versioned_file() {
        let path = temp_session_path("anti-clobber-clear");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, newer_version_json(SNAPSHOT_VERSION + 1)).unwrap();

        clear_path(&path).unwrap();

        assert!(!path.exists());
        let bak = path.with_file_name(format!("session.json.v{}.0.bak", SNAPSHOT_VERSION + 1));
        assert!(bak.exists());
    }

    fn versioned_session_json(version: u32, selected: usize) -> String {
        serde_json::json!({
            "version": version,
            "workspaces": [],
            "active": null,
            "selected": selected
        })
        .to_string()
    }

    #[test]
    fn recovery_restores_readable_backup() {
        let path = temp_session_path("recovery");
        let parent = path.parent().unwrap();
        std::fs::create_dir_all(parent).unwrap();
        let bak = parent.join(format!("session.json.v{}.0.bak", SNAPSHOT_VERSION));
        std::fs::write(&bak, versioned_session_json(SNAPSHOT_VERSION, 42)).unwrap();

        let recovered = recover_backup_to_primary(&path, |c| parse_snapshot(c).is_ok()).unwrap();

        assert_eq!(parse_snapshot(&recovered).unwrap().selected, 42);
        assert!(path.exists());
        assert!(!bak.exists());
    }

    #[test]
    fn writer_backup_name_matches_reader_scan_convention() {
        // Locks the shared `<stem>.v<N>.<n>.bak` convention: the file the writer
        // produces must satisfy the prefix/suffix the reader scans for. (A full
        // write→recover chain in one binary is impossible by design — the writer
        // only backs up files newer than this binary, which the reader's version
        // gate refuses — so we assert the naming contract directly.)
        let path = temp_session_path("writer-reader-naming");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, newer_version_json(SNAPSHOT_VERSION + 1)).unwrap();

        backup_if_newer(&path).unwrap();

        let stem = path.file_name().unwrap().to_string_lossy();
        let reader_prefix = format!("{stem}.v");
        let bak = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .find(|name| name.starts_with(&reader_prefix) && name.ends_with(".bak"))
            .expect("writer backup must match reader scan prefix/suffix");
        assert_eq!(bak, format!("session.json.v{}.0.bak", SNAPSHOT_VERSION + 1));
    }

    #[test]
    fn recovery_skips_unreadable_backup() {
        let path = temp_session_path("recovery-skip");
        let parent = path.parent().unwrap();
        std::fs::create_dir_all(parent).unwrap();
        let bak = parent.join(format!("session.json.v{}.0.bak", SNAPSHOT_VERSION + 1));
        std::fs::write(&bak, newer_version_json(SNAPSHOT_VERSION + 1)).unwrap();

        let recovered = recover_backup_to_primary(&path, |c| parse_snapshot(c).is_ok());

        assert!(recovered.is_none());
        assert!(bak.exists());
    }

    #[test]
    fn recovery_ignores_unrelated_and_version_mismatched_backups() {
        // A valid current-version primary must survive even when the directory
        // holds a same-prefix-but-malformed file and a strictly-named backup
        // whose content version disagrees with its filename. Neither qualifies,
        // so recovery is a no-op and the primary is untouched.
        let path = temp_session_path("recovery-strict-names");
        let parent = path.parent().unwrap();
        std::fs::create_dir_all(parent).unwrap();
        std::fs::write(&path, versioned_session_json(SNAPSHOT_VERSION, 1)).unwrap();
        // Shares the prefix/suffix but is not the strict `.v<N>.<n>.bak` shape.
        let malformed = parent.join("session.json.vkeep.bak");
        std::fs::write(&malformed, "{}").unwrap();
        // Strict name claiming a future version, but content is an older version.
        let mismatched = parent.join(format!("session.json.v{}.0.bak", SNAPSHOT_VERSION + 1));
        std::fs::write(&mismatched, versioned_session_json(SNAPSHOT_VERSION - 1, 5)).unwrap();

        let recovered = recover_backup_to_primary(&path, |c| parse_snapshot(c).is_ok());

        assert!(recovered.is_none());
        assert!(malformed.exists());
        assert!(mismatched.exists());
        assert_eq!(
            parse_snapshot(&std::fs::read_to_string(&path).unwrap())
                .unwrap()
                .selected,
            1
        );
    }

    #[test]
    fn backup_version_from_name_parses_strict_convention_only() {
        assert_eq!(
            backup_version_from_name("session.json", "session.json.v5.0.bak"),
            Some(5)
        );
        assert_eq!(
            backup_version_from_name("session.json", "session.json.v12.3.bak"),
            Some(12)
        );
        // Wrong stem, missing counter, non-numeric, wrong suffix → None.
        assert_eq!(
            backup_version_from_name("session.json", "other.json.v5.0.bak"),
            None
        );
        assert_eq!(
            backup_version_from_name("session.json", "session.json.v5.bak"),
            None
        );
        assert_eq!(
            backup_version_from_name("session.json", "session.json.vkeep.bak"),
            None
        );
        assert_eq!(
            backup_version_from_name("session.json", "session.json.v5.0.txt"),
            None
        );
    }

    #[test]
    fn recovery_skips_when_primary_is_current() {
        let path = temp_session_path("recovery-primary-current");
        let parent = path.parent().unwrap();
        std::fs::create_dir_all(parent).unwrap();
        std::fs::write(&path, versioned_session_json(SNAPSHOT_VERSION, 1)).unwrap();
        let bak = parent.join(format!("session.json.v{}.0.bak", SNAPSHOT_VERSION));
        std::fs::write(&bak, versioned_session_json(SNAPSHOT_VERSION, 99)).unwrap();

        let recovered = recover_backup_to_primary(&path, |c| parse_snapshot(c).is_ok());

        assert!(recovered.is_none());
    }

    #[test]
    fn recovery_prefers_highest_version_and_gcs_superseded() {
        let path = temp_session_path("recovery-best");
        let parent = path.parent().unwrap();
        std::fs::create_dir_all(parent).unwrap();
        let older = parent.join("session.json.v3.0.bak");
        let newer = parent.join(format!("session.json.v{}.0.bak", SNAPSHOT_VERSION));
        std::fs::write(&older, versioned_session_json(3, 3)).unwrap();
        std::fs::write(&newer, versioned_session_json(SNAPSHOT_VERSION, 4)).unwrap();

        let recovered = recover_backup_to_primary(&path, |c| parse_snapshot(c).is_ok()).unwrap();

        assert_eq!(parse_snapshot(&recovered).unwrap().selected, 4);
        // The superseded older-but-readable backup is garbage-collected.
        assert!(!older.exists());
        assert!(!newer.exists());
    }

    #[test]
    fn anti_clobber_history_file() {
        let (session_path, history_path) = temp_session_paths("anti-clobber-history");
        std::fs::create_dir_all(session_path.parent().unwrap()).unwrap();
        std::fs::write(&history_path, newer_version_json(SNAPSHOT_VERSION + 1)).unwrap();

        save_to_paths(&session_path, &history_path, &empty_snapshot(), None).unwrap();

        assert!(!history_path.exists());
        let bak = history_path.with_file_name(format!(
            "session-history.json.v{}.0.bak",
            SNAPSHOT_VERSION + 1
        ));
        assert!(bak.exists());
    }

    #[test]
    fn recovery_promotes_backup_over_older_primary() {
        // The genuine post-re-upgrade state: the downgraded binary left an
        // older primary, and a now-readable newer backup sits beside it (named
        // per the shared `<stem>.v<N>.<n>.bak` convention the writer produces).
        // Recovery must promote the newer backup over the older primary.
        let path = temp_session_path("recovery-over-older-primary");
        let parent = path.parent().unwrap();
        std::fs::create_dir_all(parent).unwrap();
        std::fs::write(&path, versioned_session_json(SNAPSHOT_VERSION - 1, 1)).unwrap();
        let bak = parent.join(format!("session.json.v{SNAPSHOT_VERSION}.0.bak"));
        std::fs::write(&bak, versioned_session_json(SNAPSHOT_VERSION, 7)).unwrap();

        let recovered = recover_backup_to_primary(&path, |c| parse_snapshot(c).is_ok()).unwrap();

        assert_eq!(parse_snapshot(&recovered).unwrap().selected, 7);
        assert_eq!(
            snapshot_file_version(&std::fs::read_to_string(&path).unwrap()),
            Some(SNAPSHOT_VERSION)
        );
        assert!(!bak.exists());
    }

    #[test]
    fn recovery_promotes_backup_over_corrupt_but_version_readable_primary() {
        // A primary whose version field parses but whose body is garbage must
        // not shadow a recoverable backup, even when its version is >= the
        // backup's. Recovery keys on parseability, not just the version field.
        let path = temp_session_path("recovery-over-corrupt-primary");
        let parent = path.parent().unwrap();
        std::fs::create_dir_all(parent).unwrap();
        // Valid version envelope, invalid `workspaces` payload → parse fails.
        std::fs::write(
            &path,
            format!(r#"{{"version":{SNAPSHOT_VERSION},"workspaces":"not-an-array"}}"#),
        )
        .unwrap();
        let bak = parent.join(format!("session.json.v{SNAPSHOT_VERSION}.0.bak"));
        std::fs::write(&bak, versioned_session_json(SNAPSHOT_VERSION, 7)).unwrap();

        let recovered = recover_backup_to_primary(&path, |c| parse_snapshot(c).is_ok()).unwrap();

        assert_eq!(parse_snapshot(&recovered).unwrap().selected, 7);
        assert!(!bak.exists());
    }

    #[test]
    fn save_to_paths_guards_both_files_before_writing_either() {
        // A newer session file is backed up and the older session written, while
        // a newer history file is simultaneously protected — neither newer file
        // is lost, and the session was not torn relative to the history guard.
        let (session_path, history_path) = temp_session_paths("anti-clobber-both");
        std::fs::create_dir_all(session_path.parent().unwrap()).unwrap();
        std::fs::write(&session_path, newer_version_json(SNAPSHOT_VERSION + 1)).unwrap();
        std::fs::write(&history_path, newer_version_json(SNAPSHOT_VERSION + 1)).unwrap();

        save_to_paths(
            &session_path,
            &history_path,
            &empty_snapshot(),
            Some(&history_snapshot("secret")),
        )
        .unwrap();

        let session_bak =
            session_path.with_file_name(format!("session.json.v{}.0.bak", SNAPSHOT_VERSION + 1));
        let history_bak = history_path.with_file_name(format!(
            "session-history.json.v{}.0.bak",
            SNAPSHOT_VERSION + 1
        ));
        assert!(session_bak.exists());
        assert!(history_bak.exists());
        assert_eq!(
            parse_snapshot(&std::fs::read_to_string(&session_path).unwrap())
                .unwrap()
                .version,
            SNAPSHOT_VERSION
        );
        assert!(std::fs::read_to_string(&history_path)
            .unwrap()
            .contains("secret"));
    }

    #[cfg(unix)]
    #[test]
    fn anti_clobber_protects_newer_file_through_symlink() {
        let target = temp_session_path("anti-clobber-symlink-target");
        let link = target.with_file_name("link.json");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, newer_version_json(SNAPSHOT_VERSION + 1)).unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        save_to_path(&link, &empty_snapshot()).unwrap();

        // The newer content is backed up at the resolved target, not lost.
        let bak = target.with_file_name(format!("session.json.v{}.0.bak", SNAPSHOT_VERSION + 1));
        assert!(bak.exists());
        let parsed = parse_snapshot(&std::fs::read_to_string(&target).unwrap()).unwrap();
        assert_eq!(parsed.version, SNAPSHOT_VERSION);
    }
}
