//! Keeps open tabs across restarts, saved or not. Lives in the platform data dir:
//! `~/.local/share/drafty` on Linux, `~/Library/Application Support/drafty` on macOS.

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::doc::{Document, io};

pub const INTERVAL: Duration = Duration::from_secs(30);

#[derive(Serialize, Deserialize, Default)]
struct SessionFile {
    tabs: Vec<TabEntry>,
    active: usize,
    #[serde(default)]
    grid: bool,
}

#[derive(Serialize, Deserialize)]
struct TabEntry {
    id: String,
    title: String,
    #[serde(default)]
    path: Option<PathBuf>,
    /// Had changes not yet saved to `path`.
    #[serde(default)]
    dirty: bool,
}

pub struct Restored {
    pub docs: Vec<Document>,
    pub active: usize,
    pub grid: bool,
}

pub struct Session {
    dir: Option<PathBuf>,
    pub last_save: Instant,
}

impl Session {
    pub fn new() -> Self {
        let dir =
            directories::ProjectDirs::from("", "", "drafty").map(|d| d.data_dir().to_path_buf());
        Self {
            dir,
            last_save: Instant::now(),
        }
    }

    /// Session stored in `dir` instead of the platform data dir.
    #[cfg(test)]
    pub fn at(dir: PathBuf) -> Self {
        Self {
            dir: Some(dir),
            last_save: Instant::now(),
        }
    }

    fn tabs_dir(&self) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join("tabs"))
    }

    pub fn load(&self) -> Option<Restored> {
        let dir = self.dir.as_ref()?;
        let json = std::fs::read_to_string(dir.join("session.json")).ok()?;
        let file: SessionFile = serde_json::from_str(&json)
            .inspect_err(|e| eprintln!("drafty: ignoring broken session file: {e}"))
            .ok()?;
        let tabs_dir = self.tabs_dir()?;
        let mut docs = Vec::new();
        let mut active = 0;
        for (i, tab) in file.tabs.into_iter().enumerate() {
            let tab_path = tabs_dir.join(format!("{}.drafty", tab.id));
            let loaded = std::fs::read_to_string(&tab_path)
                .map_err(|e| e.to_string())
                .and_then(|json| io::from_json(&json));
            let mut doc = match loaded {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("drafty: couldn't restore tab {}: {e}", tab.title);
                    continue;
                }
            };
            if i == file.active {
                active = docs.len();
            }
            doc.session_id = tab.id;
            doc.title = tab.title;
            doc.path = tab.path;
            // Restored content counts as saved to the session, and as saved to the file
            // unless it wasn't before quitting.
            doc.rev = 1;
            doc.session_rev = 1;
            doc.file_rev = if tab.dirty { 0 } else { 1 };
            docs.push(doc);
        }
        if docs.is_empty() {
            return None;
        }
        Some(Restored {
            docs,
            active,
            grid: file.grid,
        })
    }

    /// Write changed tabs and the tab list. Removes files of tabs that were closed.
    pub fn save(&mut self, docs: &mut [Document], active: usize, grid: bool) {
        self.last_save = Instant::now();
        let (Some(dir), Some(tabs_dir)) = (self.dir.clone(), self.tabs_dir()) else {
            return;
        };
        if let Err(e) = std::fs::create_dir_all(&tabs_dir) {
            eprintln!("drafty: can't create {}: {e}", tabs_dir.display());
            return;
        }
        for doc in docs.iter_mut() {
            if doc.rev == doc.session_rev
                && tabs_dir.join(format!("{}.drafty", doc.session_id)).exists()
            {
                continue;
            }
            let path = tabs_dir.join(format!("{}.drafty", doc.session_id));
            match io::save(doc, &path) {
                Ok(()) => doc.session_rev = doc.rev,
                Err(e) => eprintln!("drafty: autosave failed for {}: {e}", doc.title),
            }
        }
        let file = SessionFile {
            tabs: docs
                .iter()
                .map(|d| TabEntry {
                    id: d.session_id.clone(),
                    title: d.title.clone(),
                    path: d.path.clone(),
                    dirty: d.is_dirty() && d.path.is_some(),
                })
                .collect(),
            active,
            grid,
        };
        let json = serde_json::to_string_pretty(&file).expect("session serializes");
        if let Err(e) = io::write_atomic(&dir.join("session.json"), json.as_bytes()) {
            eprintln!("drafty: can't write session: {e}");
        }

        let keep: HashSet<String> = docs
            .iter()
            .map(|d| format!("{}.drafty", d.session_id))
            .collect();
        if let Ok(entries) = std::fs::read_dir(&tabs_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if !keep.contains(&name) {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }

    pub fn needs_save(&self, docs: &[Document]) -> bool {
        docs.iter().any(|d| d.rev != d.session_rev)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::{Edit, Fill, ObjectKind, Style};
    use egui::{Color32, pos2};

    #[test]
    fn save_and_restore() {
        let dir = std::env::temp_dir().join(format!("drafty-session-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut s = Session::at(dir.clone());

        let mut a = Document::new("One");
        a.apply(Edit::Add(vec![ObjectKind::Stroke {
            points: vec![pos2(0.0, 0.0), pos2(1.0, 1.0)],
            style: Style {
                color: Color32::RED,
                width: 2.0,
                fill: Fill::None,
            },
            highlighter: false,
        }]));
        let b = Document::new("Two");
        let closed = Document::new("Closed");
        let mut docs = vec![a, b, closed];
        s.save(&mut docs, 1, true);
        docs.pop();
        s.save(&mut docs, 1, true);

        let r = s.load().unwrap();
        assert_eq!(r.docs.len(), 2);
        assert_eq!(r.active, 1);
        assert!(r.grid);
        assert_eq!(r.docs[0].title, "One");
        assert_eq!(r.docs[0].objects.len(), 1);
        assert_eq!(std::fs::read_dir(dir.join("tabs")).unwrap().count(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
