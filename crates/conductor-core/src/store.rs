use crate::{domain::*, Error, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{de::DeserializeOwned, Serialize};
use std::{path::Path, sync::Mutex};

pub struct Store {
    connection: Mutex<Connection>,
}
impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut connection = Connection::open(path)?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        connection.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;",
        )?;
        let version: u32 = connection.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version > 1 {
            return Err(Error::Invalid(
                "This data was created by a newer Conductor. Upgrade the app.".into(),
            ));
        }
        if version == 0 {
            let tx = connection.transaction()?;
            tx.execute_batch(
                "CREATE TABLE records (kind TEXT NOT NULL, id TEXT NOT NULL, value TEXT NOT NULL, updated_at INTEGER NOT NULL, PRIMARY KEY(kind,id));
                 CREATE TABLE cache (path TEXT PRIMARY KEY, hash TEXT NOT NULL, value TEXT NOT NULL);
                 PRAGMA user_version=1;",
            )?;
            tx.commit()?;
        }
        // Preserve interrupted messages instead of pretending they completed.
        let store = Self {
            connection: Mutex::new(connection),
        };
        for mut conversation in store.list::<Conversation>("conversation")? {
            let mut changed = false;
            for message in &mut conversation.messages {
                if message.status == "streaming" {
                    message.status = "interrupted".into();
                    changed = true;
                }
            }
            if changed {
                store.save_conversation(&conversation)?;
            }
        }
        Ok(store)
    }

    pub fn put<T: Serialize>(&self, kind: &str, id: &str, value: &T) -> Result<()> {
        let mut value = serde_json::to_value(value)?;
        sanitize_strings(&mut value);
        self.put_json(kind, id, &value)
    }
    fn put_json(&self, kind: &str, id: &str, value: &serde_json::Value) -> Result<()> {
        let serialized = serde_json::to_string(value)?;
        let db = self.connection.lock().map_err(|_| Error::StoreLocked)?;
        db.execute("INSERT INTO records(kind,id,value,updated_at) VALUES(?1,?2,?3,?4) ON CONFLICT(kind,id) DO UPDATE SET value=excluded.value,updated_at=excluded.updated_at", params![kind,id,serialized,now()])?;
        Ok(())
    }
    pub fn get<T: DeserializeOwned>(&self, kind: &str, id: &str) -> Result<Option<T>> {
        let db = self.connection.lock().map_err(|_| Error::StoreLocked)?;
        let value: Option<String> = db
            .query_row(
                "SELECT value FROM records WHERE kind=?1 AND id=?2",
                params![kind, id],
                |r| r.get(0),
            )
            .optional()?;
        value
            .map(|s| serde_json::from_str(&s))
            .transpose()
            .map_err(Into::into)
    }
    pub fn list<T: DeserializeOwned>(&self, kind: &str) -> Result<Vec<T>> {
        let db = self.connection.lock().map_err(|_| Error::StoreLocked)?;
        let mut query =
            db.prepare("SELECT value FROM records WHERE kind=?1 ORDER BY updated_at DESC,id")?;
        let rows = query.query_map([kind], |r| r.get::<_, String>(0))?;
        rows.map(|r| Ok(serde_json::from_str(&r?)?)).collect()
    }
    pub fn remove(&self, kind: &str, id: &str) -> Result<()> {
        self.connection
            .lock()
            .map_err(|_| Error::StoreLocked)?
            .execute(
                "DELETE FROM records WHERE kind=?1 AND id=?2",
                params![kind, id],
            )?;
        Ok(())
    }
    pub fn settings(&self) -> Result<Settings> {
        Ok(self.get("settings", "global")?.unwrap_or_default())
    }
    pub fn save_settings(&self, settings: &Settings) -> Result<()> {
        if !["system", "dark", "light"].contains(&settings.theme.as_str()) {
            return Err(Error::Invalid("Unknown theme".into()));
        }
        if settings.instructions.len() > 32_000 {
            return Err(Error::Invalid("Instructions exceed 32 KB".into()));
        }
        self.put("settings", "global", settings)
    }
    pub fn save_conversation(&self, conversation: &Conversation) -> Result<()> {
        if self
            .get::<Project>("project", &conversation.project_id)?
            .is_none()
        {
            return Err(Error::Invalid("Open a project first".into()));
        }
        // Redact text, never binary encodings. Validate each message's image
        // parts before preserving their bytes; historical turns may contain
        // more images than one provider request can send together.
        for message in &conversation.messages {
            crate::providers::validate_images(std::slice::from_ref(message))?;
        }
        let mut value = serde_json::to_value(conversation)?;
        let images: Vec<_> = value["messages"]
            .as_array_mut()
            .ok_or_else(|| Error::Invalid("Invalid conversation messages".into()))?
            .iter_mut()
            .map(|message| message["images"].take())
            .collect();
        sanitize_strings(&mut value);
        for (index, images) in images.into_iter().enumerate() {
            value["messages"][index]["images"] = images;
        }
        self.put_json("conversation", &conversation.id, &value)
    }
    pub fn history(&self, project_id: Option<String>, kind: &str, summary: &str) -> Result<()> {
        let entry_id = id();
        self.put(
            "history",
            &entry_id,
            &HistoryEntry {
                id: entry_id.clone(),
                project_id,
                kind: kind.into(),
                summary: crate::context::redact(summary),
                created_at: now(),
            },
        )?;
        let db = self.connection.lock().map_err(|_| Error::StoreLocked)?;
        db.execute("DELETE FROM records WHERE kind='history' AND id NOT IN (SELECT id FROM records WHERE kind='history' ORDER BY updated_at DESC LIMIT 2000)", [])?;
        Ok(())
    }
    pub fn cache_get(&self, path: &str, hash: &str) -> Result<Option<String>> {
        Ok(self
            .connection
            .lock()
            .map_err(|_| Error::StoreLocked)?
            .query_row(
                "SELECT value FROM cache WHERE path=?1 AND hash=?2",
                params![path, hash],
                |r| r.get(0),
            )
            .optional()?)
    }
    pub fn cache_put(&self, path: &str, hash: &str, value: &str) -> Result<()> {
        let value = crate::context::redact(value);
        self.connection.lock().map_err(|_| Error::StoreLocked)?.execute("INSERT INTO cache(path,hash,value) VALUES(?1,?2,?3) ON CONFLICT(path) DO UPDATE SET hash=excluded.hash,value=excluded.value",params![path,hash,value])?;
        Ok(())
    }
    pub fn snapshot(&self, data_dir: String) -> Result<Snapshot> {
        Ok(Snapshot {
            settings: self.settings()?,
            projects: self.list("project")?,
            providers: self.list("provider")?,
            conversations: self.list("conversation")?,
            history: self.list("history")?,
            data_dir,
        })
    }
}

// Walk parsed JSON, never serialized JSON: redacting quoted strings must not
// corrupt escaping or schema structure. This also covers generic record users.
fn sanitize_strings(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::String(text) => *text = crate::context::redact(text),
        serde_json::Value::Array(items) => items.iter_mut().for_each(sanitize_strings),
        serde_json::Value::Object(map) => map.values_mut().for_each(sanitize_strings),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn redacts_nested_records_cache_and_instructions_without_corrupting_json() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let store = Store::open(&temp.path().join("state.db"))?;
        let token = "sk-abcdefghijklmnopqrstuvwxyz123456";
        let value = serde_json::json!({"nested":[{"text":format!("quoted \"{token}\"\npassword=very-secret")}]});
        store.put("test", "secret-fixture", &value)?;
        let saved: serde_json::Value = store
            .get("test", "secret-fixture")?
            .ok_or_else(|| Error::Invalid("Record missing".into()))?;
        assert!(!saved.to_string().contains(token));
        assert!(!saved.to_string().contains("very-secret"));
        assert!(saved["nested"][0]["text"].as_str().is_some());
        store.cache_put("x", "h", token)?;
        assert!(!store
            .cache_get("x", "h")?
            .unwrap_or_default()
            .contains(token));
        store.save_settings(&Settings {
            instructions: token.into(),
            ..Default::default()
        })?;
        assert!(!store.settings()?.instructions.contains(token));
        Ok(())
    }
    #[test]
    fn history_identity_matches_record_and_can_be_removed() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let store = Store::open(&temp.path().join("state.db"))?;
        store.history(None, "test", "Checked")?;
        let history = store.list::<HistoryEntry>("history")?;
        assert_eq!(history.len(), 1);
        store.remove("history", &history[0].id)?;
        assert!(store.list::<HistoryEntry>("history")?.is_empty());
        Ok(())
    }
    #[test]
    fn refuses_future_schemas_and_corruption_without_overwriting_data() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let future = temp.path().join("future.db");
        let db = Connection::open(&future)?;
        db.execute_batch("PRAGMA user_version=999;")?;
        drop(db);
        assert!(matches!(Store::open(&future), Err(Error::Invalid(_))));
        let reopened = Connection::open(&future)?;
        assert_eq!(
            reopened.query_row::<u32, _, _>("PRAGMA user_version", [], |r| r.get(0))?,
            999
        );
        let corrupt = temp.path().join("corrupt.db");
        std::fs::write(&corrupt, b"this is not a SQLite database")?;
        assert!(Store::open(&corrupt).is_err());
        assert_eq!(std::fs::read(&corrupt)?, b"this is not a SQLite database");
        Ok(())
    }
    #[test]
    fn persists_settings_and_recovers_interrupted_work() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("state.db");
        let store = Store::open(&path)?;
        let settings = Settings {
            setup_complete: true,
            ..Default::default()
        };
        store.save_settings(&settings)?;
        let project = Project {
            id: id(),
            name: "P".into(),
            path: temp.path().display().to_string(),
            kind: "folder".into(),
            git: false,
            opened_at: now(),
        };
        store.put("project", &project.id, &project)?;
        let mut message = Message::new(Role::Assistant, "Partial response".into());
        message.status = "streaming".into();
        let c = Conversation {
            id: id(),
            project_id: project.id,
            title: "Test".into(),
            mode: Mode::Chat,
            provider_id: None,
            model_id: None,
            effort: None,
            messages: vec![message],
            updated_at: now(),
        };
        store.save_conversation(&c)?;
        drop(store);
        let reopened = Store::open(&path)?;
        assert!(reopened.settings()?.setup_complete);
        let restored: Conversation = reopened
            .get("conversation", &c.id)?
            .ok_or_else(|| Error::Invalid("Missing conversation".into()))?;
        assert_eq!(restored.messages[0].status, "interrupted");
        assert_eq!(restored.messages[0].text, "Partial response");
        Ok(())
    }
    #[test]
    fn parameterized_records_and_hash_invalidation() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let store = Store::open(&temp.path().join("state.db"))?;
        let name = "'; DROP TABLE records;--";
        store.put("test", name, &42)?;
        assert_eq!(store.get::<u32>("test", name)?, Some(42));
        store.cache_put("x", "a", "summary")?;
        assert_eq!(store.cache_get("x", "b")?, None);
        assert_eq!(store.cache_get("x", "a")?, Some("summary".into()));
        Ok(())
    }
    #[test]
    fn conversation_images_round_trip_without_text_redaction_corrupting_bytes() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("images.db");
        let store = Store::open(&path)?;
        let project = Project {
            id: id(),
            name: "P".into(),
            path: temp.path().display().to_string(),
            kind: "folder".into(),
            git: false,
            opened_at: now(),
        };
        store.put("project", &project.id, &project)?;
        // Valid base64 with a PNG signature. An arbitrary binary payload can
        // coincidentally encode a token-shaped substring; it is not text.
        use base64::Engine;
        let binary = "iVBORw0KGgoA/AKIA1234567890123456/AA";
        assert!(base64::engine::general_purpose::STANDARD
            .decode(binary)
            .unwrap()
            .starts_with(b"\x89PNG\r\n\x1a\n"));
        let token = "sk-abcdefghijklmnopqrstuvwxyz123456";
        assert_ne!(crate::context::redact(binary), binary);
        let mut message = Message::new(Role::User, token.into());
        message.images.push(ImageAttachment {
            mime_type: "image/png".into(),
            data: binary.into(),
        });
        let conversation = Conversation {
            id: id(),
            project_id: project.id,
            title: "Images".into(),
            mode: Mode::Chat,
            provider_id: None,
            model_id: None,
            effort: None,
            messages: vec![message],
            updated_at: now(),
        };
        store.save_conversation(&conversation)?;
        drop(store);
        let store = Store::open(&path)?;
        let restored: Conversation = store
            .get("conversation", &conversation.id)?
            .ok_or_else(|| Error::Invalid("Missing image conversation".into()))?;
        assert_eq!(restored.messages[0].images[0].data, binary);
        assert!(!restored.messages[0].text.contains(token));
        let mut invalid = restored.clone();
        invalid.messages[0].images[0].data = token.into();
        assert!(matches!(
            store.save_conversation(&invalid),
            Err(Error::Invalid(_))
        ));
        invalid.messages[0].images[0].data = binary.into();
        invalid.messages[0].images[0].mime_type = "application/octet-stream".into();
        assert!(matches!(
            store.save_conversation(&invalid),
            Err(Error::Invalid(_))
        ));
        let unchanged: Conversation = store.get("conversation", &conversation.id)?.unwrap();
        assert_eq!(unchanged.messages[0].images, restored.messages[0].images);
        let mut history = restored.clone();
        history.messages = vec![restored.messages[0].clone(); 12];
        store.save_conversation(&history)?;
        let history: Conversation = store.get("conversation", &history.id)?.unwrap();
        assert_eq!(history.messages.len(), 12);
        assert!(history
            .messages
            .iter()
            .all(|message| message.images[0].data == binary));
        Ok(())
    }
}
