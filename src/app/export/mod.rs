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

/// Highest `-N` suffix tried when a default file name is taken, so a directory
/// full of exports ends in an error instead of an unbounded search.
const MAX_DEFAULT_NAME_ATTEMPTS: u32 = 1000;
/// UTC basic ISO-8601 with a literal `T` and `Z`, as the default name documents.
const EXPORT_STAMP_FORMAT: &str = "%Y%m%dT%H%M%SZ";

/// The one `/export` in flight: where it writes and the history received so far.
pub(crate) struct PendingExport {
    request_id: String,
    session_id: String,
    destination: Destination,
    updates: Vec<model::SessionUpdate>,
    omitted_updates: u64,
}

/// Where the file goes. An explicit path is used as given and is an error when
/// taken; a default name is `<stem>.md` and moves to `<stem>-2.md`, `<stem>-3.md`,
/// ... when taken.
#[derive(Debug, PartialEq, Eq)]
enum Destination {
    Explicit(PathBuf),
    Default { dir: PathBuf, stem: String },
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
/// Refuses before any bridge work when an explicit destination already exists.
/// A taken default name is not refused: the write picks the first free suffix.
pub(crate) fn begin(app: &mut App, session_id: &str, requested: &str) -> Result<String, String> {
    begin_at(app, session_id, requested, chrono::Utc::now())
}

/// [`begin`] with the export instant given, so the default name is testable.
fn begin_at(
    app: &mut App,
    session_id: &str,
    requested: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<String, String> {
    let destination = resolve_destination(
        requested,
        Path::new(&app.cwd_raw),
        &default_stem(session_id, now),
        dirs::home_dir().as_deref(),
    );
    if let Destination::Explicit(path) = &destination {
        ensure_absent(path)?;
    }
    let request_id = uuid::Uuid::new_v4().to_string();
    app.history_export = Some(Box::new(PendingExport {
        request_id: request_id.clone(),
        session_id: session_id.to_owned(),
        destination,
        updates: Vec::new(),
        omitted_updates: 0,
    }));
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
    let written = match &pending.destination {
        Destination::Explicit(path) => write_new_file(path, &content),
        Destination::Default { dir, stem } => write_default_file(dir, stem, &content),
    };
    match written {
        Ok(path) => ExportProgress::Written(path),
        Err(message) => ExportProgress::Failed(message),
    }
}

/// `export-<session-id>-<UTC stamp>`: the full session id and no title, so the
/// name needs no sanitising beyond what a session id already is.
fn default_stem(session_id: &str, now: chrono::DateTime<chrono::Utc>) -> String {
    format!("export-{session_id}-{}", now.format(EXPORT_STAMP_FORMAT))
}

/// Attempt `n` of a default name: `<stem>.md`, then `<stem>-2.md`, `<stem>-3.md`, ...
fn numbered_name(stem: &str, attempt: u32) -> String {
    if attempt <= 1 { format!("{stem}.md") } else { format!("{stem}-{attempt}.md") }
}

/// Relative paths are relative to the session directory; a leading `~` is the home directory.
fn resolve_destination(
    requested: &str,
    cwd: &Path,
    default_stem: &str,
    home: Option<&Path>,
) -> Destination {
    let requested = requested.trim();
    if requested.is_empty() {
        return Destination::Default { dir: cwd.to_path_buf(), stem: default_stem.to_owned() };
    }
    let expanded = match (requested, home) {
        ("~", Some(home)) => home.to_path_buf(),
        (path, Some(home)) => match path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\")) {
            Some(rest) => home.join(rest),
            None => PathBuf::from(path),
        },
        (path, None) => PathBuf::from(path),
    };
    Destination::Explicit(if expanded.is_absolute() { expanded } else { cwd.join(expanded) })
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
    let file = open_new(path).map_err(|error| open_error(path, &error))?;
    finish_write(file, path, content)
}

/// Write a default-named export, taking the first free `-N` suffix.
///
/// Each candidate is claimed with `create_new` rather than checked first, so two
/// exports racing for the same name cannot both win it.
fn write_default_file(dir: &Path, stem: &str, content: &str) -> Result<PathBuf, String> {
    for attempt in 1..=MAX_DEFAULT_NAME_ATTEMPTS {
        let path = dir.join(numbered_name(stem, attempt));
        match open_new(&path) {
            Ok(file) => return finish_write(file, &path, content),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(error) => return Err(open_error(&path, &error)),
        }
    }
    Err(format!(
        "Not exporting: {stem}.md and its -2 to -{MAX_DEFAULT_NAME_ATTEMPTS} variants in {} already exist.",
        dir.display()
    ))
}

fn open_new(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new().write(true).create_new(true).open(path)
}

fn open_error(path: &Path, error: &std::io::Error) -> String {
    match error.kind() {
        ErrorKind::AlreadyExists => already_exists(path),
        ErrorKind::NotFound => {
            format!("Cannot export to {}: its directory does not exist.", path.display())
        }
        _ => format!("Cannot export to {}: {error}", path.display()),
    }
}

fn finish_write(mut file: std::fs::File, path: &Path, content: &str) -> Result<PathBuf, String> {
    if let Err(error) = file.write_all(content.as_bytes()) {
        // Do not leave a half-written export behind for the next run to refuse over.
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err(format!("Failed to write {}: {error}", path.display()));
    }
    Ok(std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf()))
}
