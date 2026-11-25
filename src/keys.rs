use crate::library::Library;
use anyhow::{Result, ensure};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ring::{
    digest::{SHA256, digest},
    rand::{SecureRandom, SystemRandom},
};
use rusqlite::params;

pub const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS inference_keys(id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, digest BLOB NOT NULL UNIQUE, created TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);";
#[derive(serde::Serialize)]
pub struct Key {
    pub id: i64,
    pub name: String,
    pub created: String,
}
impl Library {
    pub fn create_key(&self, name: &str) -> Result<String> {
        let name = name.trim();
        ensure!(
            !name.is_empty() && name.len() <= 80 && !name.chars().any(char::is_control),
            "Enter a key name (1–80 bytes)"
        );
        let mut bytes = [0; 32];
        SystemRandom::new()
            .fill(&mut bytes)
            .map_err(|_| anyhow::anyhow!("System randomness unavailable"))?;
        let token = format!("lum_{}", URL_SAFE_NO_PAD.encode(bytes));
        self.db()?.execute(
            "INSERT INTO inference_keys(name,digest) VALUES(?,?)",
            params![name, digest(&SHA256, token.as_bytes()).as_ref()],
        )?;
        Ok(token)
    }
    pub fn keys(&self) -> Result<Vec<Key>> {
        let db = self.db()?;
        let mut q = db.prepare("SELECT id,name,created FROM inference_keys ORDER BY id DESC")?;
        Ok(q.query_map([], |row| {
            Ok(Key {
                id: row.get(0)?,
                name: row.get(1)?,
                created: row.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?)
    }
    pub fn revoke_key(&self, id: i64) -> Result<()> {
        ensure!(
            self.db()?
                .execute("DELETE FROM inference_keys WHERE id=?", [id])?
                == 1,
            "Key no longer exists"
        );
        Ok(())
    }
    pub fn permits(&self, token: &str) -> Result<bool> {
        if token.len() != 47 || !token.starts_with("lum_") {
            return Ok(false);
        }
        Ok(self.db()?.query_row(
            "SELECT EXISTS(SELECT 1 FROM inference_keys WHERE digest=?)",
            [digest(&SHA256, token.as_bytes()).as_ref()],
            |r| r.get(0),
        )?)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keys_survive_reopen_without_plaintext() {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::open(dir.path()).unwrap();
        let secret = lib.create_key("editor").unwrap();
        let lib = Library::open(dir.path()).unwrap();
        assert!(lib.permits(&secret).unwrap());
        let data = std::fs::read(dir.path().join("library.sqlite")).unwrap();
        assert!(!data.windows(secret.len()).any(|s| s == secret.as_bytes()));
        lib.revoke_key(lib.keys().unwrap()[0].id).unwrap();
        assert!(!lib.permits(&secret).unwrap());
    }
}
