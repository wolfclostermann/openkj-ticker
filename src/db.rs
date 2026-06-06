use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Serialize, Clone)]
pub struct Singer {
    pub name: String,
    pub next_song_artist: Option<String>,
    pub next_song_title: Option<String>,
    pub is_current: bool,
}

#[derive(Debug, Serialize, Clone)]
pub struct KaraokeState {
    /// Singer currently up / singing (from currentRotationPosition in the INI).
    pub current_singer: Option<Singer>,
    /// The singer immediately after the current one in rotation order.
    pub next_up: Option<Singer>,
    /// Full rotation in position order (all singers).
    pub rotation: Vec<Singer>,
    /// The configured display limit passed through to the client.
    pub singer_count: usize,
    /// True when the current singer's most recently started song is still in their unplayed queue.
    pub is_playing: bool,
    pub status: String,
}

/// Read `currentRotationPosition` from the OpenKJ INI file.
/// Qt writes top-level keys under a `[General]` section in its INI format.
pub fn read_current_singer_id(data_dir: &Path) -> Option<i64> {
    for ini_name in &["openkj2.ini", "openkj2-unstable.ini"] {
        let path = data_dir.join(ini_name);
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        for line in content.lines() {
            let line = line.trim();
            let parts: Vec<&str> = line.splitn(2, '=').collect();
            if parts.len() == 2
                && parts[0]
                    .trim()
                    .eq_ignore_ascii_case("currentRotationPosition")
            {
                if let Ok(id) = parts[1].trim().parse::<i64>() {
                    return Some(id);
                }
            }
        }
    }
    None
}

pub fn query_state(data_dir: &Path, singer_count: usize) -> Result<KaraokeState> {
    let db_path = data_dir.join("openkj.sqlite");

    if !db_path.exists() {
        return Ok(KaraokeState {
            current_singer: None,
            next_up: None,
            rotation: vec![],
            singer_count,
            is_playing: false,
            status: "database_not_found".to_string(),
        });
    }

    let conn = Connection::open_with_flags(
        &db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .context("Failed to open OpenKJ database")?;

    // Allow up to 500 ms for a busy lock (OpenKJ holds a write lock briefly on saves).
    conn.busy_timeout(std::time::Duration::from_millis(500))?;

    let current_singer_id = read_current_singer_id(data_dir);

    // Load all singers in rotation order.
    let mut stmt = conn
        .prepare("SELECT singerid, name FROM rotationSingers ORDER BY position")
        .context("Failed to prepare rotation query")?;

    let singer_rows: Vec<(i64, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .filter_map(|r| r.ok())
        .collect();

    let mut rotation: Vec<Singer> = Vec::new();
    let mut current_idx: Option<usize> = None;

    for (i, (singer_id, name)) in singer_rows.iter().enumerate() {
        let is_current = current_singer_id
            .map(|id| id == *singer_id)
            .unwrap_or(false);

        // First unplayed song in this singer's queue.
        let (artist, title) = conn
            .query_row(
                "SELECT d.artist, d.title \
                 FROM dbsongs d \
                 JOIN queuesongs q ON d.songid = q.song \
                 WHERE q.singer = ?1 AND q.played = 0 \
                 ORDER BY q.position \
                 LIMIT 1",
                [singer_id],
                |row| {
                    Ok((
                        row.get::<_, Option<String>>(0)?,
                        row.get::<_, Option<String>>(1)?,
                    ))
                },
            )
            .unwrap_or((None, None));

        if is_current {
            current_idx = Some(i);
        }

        rotation.push(Singer {
            name: name.clone(),
            next_song_artist: artist,
            next_song_title: title,
            is_current,
        });
    }

    let current_singer = current_idx.map(|i| rotation[i].clone());

    // A song is "playing" if the most recently started song (historySongs.lastplay)
    // for the current singer is still present in their unplayed queue.
    let is_playing = match (&current_singer, current_singer_id) {
        (Some(singer), Some(singer_id)) => {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) \
                     FROM (SELECT songid FROM historySongs \
                           WHERE historySinger = ?1 \
                           ORDER BY lastplay DESC LIMIT 1) recent \
                     JOIN queuesongs q \
                       ON q.song = recent.songid \
                      AND q.singer = ?2 \
                      AND q.played = 0",
                    rusqlite::params![singer.name, singer_id],
                    |row| row.get(0),
                )
                .unwrap_or(0);
            count > 0
        }
        _ => false,
    };

    let next_up = match current_idx {
        Some(i) if i + 1 < rotation.len() => Some(rotation[i + 1].clone()),
        // Wrap around to the first singer if current is last.
        Some(_) if rotation.len() > 1 => Some(rotation[0].clone()),
        // No current singer — first in rotation is "next up".
        None => rotation.first().cloned(),
        _ => None,
    };

    Ok(KaraokeState {
        current_singer,
        next_up,
        rotation,
        singer_count,
        is_playing,
        status: "ok".to_string(),
    })
}
