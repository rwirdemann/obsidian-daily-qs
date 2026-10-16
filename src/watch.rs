//! Poll today's daily note (and optionally the inbox note) and emit JSON
//! snapshots when they change.

use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use chrono::Local;

use crate::config::Vault;
use crate::status::Snapshot;
use crate::todos::read_snapshot_filtered;

const POLL: Duration = Duration::from_secs(1);

/// Stream snapshots: one immediately, then whenever path/mtime/content changes.
/// With `with_inbox`, inbox snapshots (`"inbox": true`) are interleaved so
/// external edits to the inbox note reach the widget too.
pub fn watch(
    cli_vault: Option<PathBuf>,
    cli_archive: Option<String>,
    heading: Option<String>,
    with_inbox: bool,
) {
    let mut last_key: Option<String> = None;
    let mut last_inbox_key: Option<String> = None;
    loop {
        let snap = current_snapshot(cli_vault.clone(), cli_archive.clone(), heading.as_deref());
        if !emit_if_changed(&snap, &mut last_key) {
            return;
        }
        // Inbox errors are not streamed: they carry no inbox flag and would
        // be taken for a today snapshot.
        if with_inbox
            && let Some(snap) = inbox_snapshot(cli_vault.clone(), cli_archive.clone())
            && !emit_if_changed(&snap, &mut last_inbox_key)
        {
            return;
        }
        thread::sleep(POLL);
    }
}

/// Emit `snap` when its key differs from `last_key`; false once stdout is gone.
fn emit_if_changed(snap: &Snapshot, last_key: &mut Option<String>) -> bool {
    let key = snapshot_key(snap);
    if last_key.as_ref() == Some(&key) {
        return true;
    }
    if !emit(snap) {
        return false;
    }
    *last_key = Some(key);
    true
}

/// The inbox note's snapshot, or `None` when it cannot be read.
fn inbox_snapshot(cli_vault: Option<PathBuf>, cli_archive: Option<String>) -> Option<Snapshot> {
    let vault = Vault::resolve(cli_vault, cli_archive).ok()?.with_inbox(true);
    read_snapshot_filtered(&vault, Local::now().date_naive(), None).ok()
}

pub fn current_snapshot(
    cli_vault: Option<PathBuf>,
    cli_archive: Option<String>,
    heading: Option<&str>,
) -> Snapshot {
    match Vault::resolve(cli_vault, cli_archive) {
        Ok(vault) => {
            let date = Local::now().date_naive();
            match read_snapshot_filtered(&vault, date, heading) {
                Ok(snap) => snap,
                Err(err) => Snapshot::error_with_code(err.to_string(), err.error_code()),
            }
        }
        Err(err) => Snapshot::error_with_code(err.to_string(), err.error_code()),
    }
}

fn snapshot_key(snap: &Snapshot) -> String {
    let body = serde_json::to_string(snap).unwrap_or_default();
    if let (Some(path), Some(true)) = (snap.path.as_ref(), snap.exists) {
        let mtime = fs::metadata(path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let len = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        format!("ok:{path}:{mtime}:{len}:{body}")
    } else {
        body
    }
}

fn emit(snap: &Snapshot) -> bool {
    let line = serde_json::to_string(snap).expect("snapshot serializes");
    let mut out = io::stdout().lock();
    let mut broken = writeln!(out, "{line}").is_err();
    broken |= out.flush().is_err();
    !broken
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status::State;

    fn snap_with_carry(carry: usize) -> Snapshot {
        Snapshot {
            state: State::Ok,
            date: Some("2026-08-21".into()),
            path: Some("/nonexistent/vault/Daily/2026-08-21.md".into()),
            exists: Some(true),
            open_count: Some(0),
            done_count: Some(0),
            todos: Some(Vec::new()),
            obsidian_uri: None,
            carry_over_count: Some(carry),
            is_today: Some(true),
            inbox: None,
            template_name: None,
            created_from_template: None,
            error_code: None,
            error: None,
        }
    }

    #[test]
    fn key_changes_when_only_carry_over_count_changes() {
        assert_ne!(
            snapshot_key(&snap_with_carry(0)),
            snapshot_key(&snap_with_carry(3))
        );
        assert_eq!(
            snapshot_key(&snap_with_carry(2)),
            snapshot_key(&snap_with_carry(2))
        );
    }
}
