//! MCode TUI and desktop share the local runtime database.
use crate::session_manager::{SessionMessage, SessionMeta};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub(crate) fn database_path() -> PathBuf {
    crate::mcode_config::data_dir().join("v2/sqlite/runtime-state.sqlite")
}

pub(crate) fn open_database() -> rusqlite::Result<Connection> {
    Connection::open_with_flags(database_path(), OpenFlags::SQLITE_OPEN_READ_ONLY)
}

pub fn scan_sessions() -> Vec<SessionMeta> {
    let Ok(data_dir) = std::path::absolute(crate::mcode_config::data_dir()) else {
        return vec![];
    };
    let database = data_dir.join("v2/sqlite/runtime-state.sqlite");
    if !database.exists() {
        return vec![];
    }
    match Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .and_then(|conn| scan(&conn, &data_dir))
    {
        Ok(sessions) => sessions,
        Err(error) => {
            log::warn!("Cannot read MCode sessions: {error}");
            vec![]
        }
    }
}

fn scan(conn: &Connection, data_dir: &Path) -> rusqlite::Result<Vec<SessionMeta>> {
    #[cfg(not(windows))]
    let command = format!(
        "env MINIMAX_DATA_DIR={} mcode",
        crate::session_manager::terminal::shell_escape(&data_dir.to_string_lossy())
    );
    #[cfg(windows)]
    let command = format!(
        "$env:MINIMAX_DATA_DIR = '{}'; mcode",
        data_dir.to_string_lossy().replace('\'', "''")
    );
    let mut query = conn.prepare(
        "SELECT session_id, title, workspace_dir, created_at_ms, updated_at_ms
         FROM local_runtime_sessions WHERE visibility <> 'hidden' AND archived = 0
         AND parent_session_id IS NULL AND session_kind NOT IN ('peek', 'channel', 'cron')
         ORDER BY updated_at_ms DESC",
    )?;
    let rows = query.query_map([], |row| {
        let id: String = row.get(0)?;
        Ok(SessionMeta {
            provider_id: "mcode".into(),
            source_path: Some(format!("mcode:{id}")),
            resume_command: id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
                .then(|| format!("{command} --session {id}")),
            session_id: id,
            title: row.get(1)?,
            summary: None,
            project_dir: row.get(2)?,
            created_at: row.get(3)?,
            last_active_at: row.get(4)?,
        })
    })?;
    rows.collect()
}

pub fn load_messages(source: &str) -> Result<Vec<SessionMessage>, String> {
    let id = source
        .strip_prefix("mcode:")
        .ok_or("Invalid MCode session source")?;
    let conn = open_database().map_err(|e| e.to_string())?;
    read_messages(&conn, id).map_err(|e| e.to_string())
}

fn read_messages(conn: &Connection, id: &str) -> rusqlite::Result<Vec<SessionMessage>> {
    let mut query = conn.prepare(
        "WITH migrated AS (
            SELECT 1 FROM local_runtime_message_row_migrations WHERE session_id = ?1
         ), display AS (
            SELECT role, data_json, created_at_ms, id AS sequence
            FROM local_runtime_message_rows
            WHERE session_id = ?1 AND EXISTS (SELECT 1 FROM migrated)
            UNION ALL
            SELECT json_extract(message.value, '$.role'), message.value, NULL, message.key
            FROM local_runtime_messages, json_each(display_messages_json) AS message
            WHERE session_id = ?1 AND NOT EXISTS (SELECT 1 FROM migrated)
         )
         SELECT role, data_json, created_at_ms FROM display
         WHERE role IN ('user', 'assistant') ORDER BY sequence",
    )?;
    let rows = query.query_map([id], |row| {
        let data: String = row.get(1)?;
        let value: Value = serde_json::from_str(&data).unwrap_or_default();
        Ok(SessionMessage {
            role: row.get(0)?,
            content: super::utils::extract_text(&value["msg_content"]),
            ts: row.get::<_, Option<i64>>(2)?.or_else(|| {
                let time = value
                    .get("timestamp")
                    .filter(|v| !v.is_null())
                    .or_else(|| value.get("created_at"))?;
                time.as_f64()
                    .or_else(|| time.as_str()?.parse::<f64>().ok())
                    .filter(|time| time.is_finite())
                    .map(|time| time.floor() as i64)
            }),
        })
    })?;
    rows.filter_map(|r| match r {
        Ok(m) if m.content.trim().is_empty() => None,
        other => Some(other),
    })
    .collect()
}
