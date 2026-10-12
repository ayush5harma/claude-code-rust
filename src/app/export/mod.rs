// SPDX-License-Identifier: Apache-2.0

//! `/export`: save the session's complete saved history as a Markdown file.
//!
//! The history comes from the bridge's resume translation of the saved
//! transcript, not from the in-memory transcript, which retention caps. This
//! module owns the destination, the one pending request and the file write;
//! `markdown` owns the presentation.

mod markdown;
#[cfg(test)]
mod tests;

use crate::agent::events::HistoryChunk;
use crate::agent::model;
use crate::app::App;
use std::io::{ErrorKind, Write as _};
use std::path::{Path, PathBuf};

/// Longest slug taken from a session title for the default file name.
const MAX_SLUG_CHARS: usize = 40;
/// Characters of the session id used when there is no usable title.
const SHORT_ID_CHARS: usize = 8;

/// The one `/export` in flight: where it writes and the history received so far.
pub(crate) struct PendingExport {
    request_id: String,
    session_id: String,
    destination: PathBuf,
    updates: Vec<model::SessionUpdate>,
    omitted_updates: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ExportProgress {
    /// No export is waiting for this chunk (another request, or the session changed).
    Ignored,
    Collecting,
    Failed(String),
    Written(PathBuf),
}

/// Choose the destination and record the request; returns the request id to send.
///
/// Refuses before any bridge work when the file already exists.
pub(crate) fn begin(app: &mut App, session_id: &str, requested: &str) -> Result<String, String> {
    let now = chrono::Local::now();
    let default_name = default_file_name(
        app.session_runtime.session_title.as_deref(),
        session_id,
        &now.format("%Y%m%d-%H%M%S").to_string(),
    );
    let destination = resolve_destination(
        requested,
        Path::new(&app.cwd_raw),
        &default_name,
        dirs::home_dir().as_deref(),
    );
    ensure_absent(&destination)?;
    let request_id = uuid::Uuid::new_v4().to_string();
    app.history_export = Some(PendingExport {
        request_id: request_id.clone(),
        session_id: session_id.to_owned(),
        destination,
        updates: Vec::new(),
        omitted_updates: 0,
    });
    Ok(request_id)
}

/// Fold one history chunk into the pending export and write the file after the last.
pub(crate) fn apply_chunk(app: &mut App, chunk: HistoryChunk) -> ExportProgress {
    let HistoryChunk { request_id, updates, last, omitted_updates, error } = chunk;
    let Some(pending) = app.history_export.as_mut() else {
        return ExportProgress::Ignored;
    };
    if request_id.as_deref() != Some(pending.request_id.as_str()) {
        return ExportProgress::Ignored;
    }
    if let Some(error) = error {
        app.history_export = None;
        return ExportProgress::Failed(error);
    }
    pending.updates.extend(updates);
    pending.omitted_updates = pending.omitted_updates.saturating_add(omitted_updates);
    if !last {
        return ExportProgress::Collecting;
    }
    let Some(pending) = app.history_export.take() else {
        return ExportProgress::Ignored;
    };
    let exported_at = chrono::Local::now().format("%Y-%m-%d %H:%M:%S %:z").to_string();
    let model_name = app
        .session_runtime
        .current_model
        .as_ref()
        .map(|current| current.display_name_long.as_str())
        .filter(|name| !name.is_empty());
    let header = markdown::Header {
        title: app.session_runtime.session_title.as_deref(),
        session_id: &pending.session_id,
        cwd: &app.cwd_raw,
        model: model_name,
        exported_at: &exported_at,
    };
    let content = markdown::render(&header, &pending.updates, pending.omitted_updates);
    match write_new_file(&pending.destination, &content) {
        Ok(path) => ExportProgress::Written(path),
        Err(message) => ExportProgress::Failed(message),
    }
}

/// `claude-rs-<title or short id>-<stamp>.md`, safe on every platform's file system.
fn default_file_name(title: Option<&str>, session_id: &str, stamp: &str) -> String {
    let slug = title.map(slugify).filter(|slug| !slug.is_empty());
    let name = slug.unwrap_or_else(|| session_id.chars().take(SHORT_ID_CHARS).collect());
    format!("claude-rs-{name}-{stamp}.md")
}

fn slugify(title: &str) -> String {
    let mut slug = String::new();
    for ch in title.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug: String = slug.trim_matches('-').chars().take(MAX_SLUG_CHARS).collect();
    slug.trim_end_matches('-').to_owned()
}

/// Relative paths are relative to the session directory; a leading `~` is the home directory.
fn resolve_destination(
    requested: &str,
    cwd: &Path,
    default_name: &str,
    home: Option<&Path>,
) -> PathBuf {
    let requested = requested.trim();
    if requested.is_empty() {
        return cwd.join(default_name);
    }
    let expanded = match (requested, home) {
        ("~", Some(home)) => home.to_path_buf(),
        (path, Some(home)) => match path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\")) {
            Some(rest) => home.join(rest),
            None => PathBuf::from(path),
        },
        (path, None) => PathBuf::from(path),
    };
    if expanded.is_absolute() { expanded } else { cwd.join(expanded) }
}

fn ensure_absent(path: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Err(already_exists(path)),
        Err(_) => Ok(()),
    }
}

fn already_exists(path: &Path) -> String {
    format!("Not exporting: {} already exists and will not be overwritten.", path.display())
}

/// Create `path` exclusively and write `content`; creates no directories.
///
/// `create_new` is the real overwrite guard, since the file can appear after the
/// early check in [`begin`].
fn write_new_file(path: &Path, content: &str) -> Result<PathBuf, String> {
    let mut file =
        std::fs::OpenOptions::new().write(true).create_new(true).open(path).map_err(|error| {
            match error.kind() {
                ErrorKind::AlreadyExists => already_exists(path),
                ErrorKind::NotFound => {
                    format!("Cannot export to {}: its directory does not exist.", path.display())
                }
                _ => format!("Cannot export to {}: {error}", path.display()),
            }
        })?;
    if let Err(error) = file.write_all(content.as_bytes()) {
        // Do not leave a half-written export behind for the next run to refuse over.
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err(format!("Failed to write {}: {error}", path.display()));
    }
    Ok(std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf()))
}
