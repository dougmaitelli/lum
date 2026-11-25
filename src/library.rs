use std::path::{Path, PathBuf};

use anyhow::{Result, ensure};
use rusqlite::{Connection, params};

#[derive(Clone)]
pub struct Library(pub PathBuf);

#[derive(Debug, serde::Serialize)]
pub struct Model {
    pub alias: String,
    pub label: String,
    pub context_limit: Option<i32>,
    pub repo: String,
    pub preset: String,
    pub revision: String,
    pub state: String,
    pub desired: bool,
    pub pinned: bool,
    pub error: String,
}

pub fn slug(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.as_bytes()[0].is_ascii_alphanumeric()
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
}
pub fn valid_repo(s: &str) -> bool {
    s.split_once('/').is_some_and(|(owner, r)| {
        slug(owner)
            && !matches!(
                owner,
                "datasets" | "spaces" | "collections" | "models" | "api"
            )
            && !r.is_empty()
            && r.len() <= 96
            && r.as_bytes()[0].is_ascii_alphanumeric()
            && r.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
    })
}
impl Library {
    pub fn open(root: &Path) -> Result<Self> {
        std::fs::create_dir_all(root.join("models"))?;
        let lib = Self(root.to_owned());
        let db = lib.db()?;
        db.execute_batch("CREATE TABLE IF NOT EXISTS models(alias TEXT PRIMARY KEY, repo TEXT NOT NULL, preset TEXT NOT NULL, revision TEXT NOT NULL DEFAULT '', state TEXT NOT NULL, desired INTEGER NOT NULL DEFAULT 0, error TEXT NOT NULL DEFAULT ''); UPDATE models SET state='failed',error='Download interrupted. Remove and download again.' WHERE state IN ('queued','downloading');")?;
        db.execute_batch("CREATE TABLE IF NOT EXISTS model_settings(alias TEXT PRIMARY KEY REFERENCES models(alias) ON DELETE CASCADE, label TEXT NOT NULL, context_limit INTEGER CHECK(context_limit > 0));")?;
        db.execute_batch("CREATE TABLE IF NOT EXISTS model_pins(alias TEXT NOT NULL PRIMARY KEY REFERENCES models(alias) ON DELETE CASCADE);")?;
        db.execute_batch(crate::keys::SCHEMA)?;
        Ok(lib)
    }
    /// Run synchronous SQLite work off the HTTP/event-loop thread.
    pub async fn run<T: Send + 'static>(
        &self,
        f: impl FnOnce(&Self) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let lib = self.clone();
        tokio::task::spawn_blocking(move || f(&lib)).await?
    }
    pub async fn for_alias<T: Send + 'static>(
        &self,
        alias: &str,
        f: impl FnOnce(&Self, &str) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let alias = alias.to_owned();
        self.run(move |lib| f(lib, &alias)).await
    }
    pub(crate) fn db(&self) -> Result<Connection> {
        let db = Connection::open(self.0.join("library.sqlite"))?;
        db.pragma_update(None, "foreign_keys", true)?;
        Ok(db)
    }
    pub fn package(&self, a: &str) -> PathBuf {
        self.0.join("models").join(a)
    }
    pub fn staging(&self, a: &str) -> PathBuf {
        self.0.join("models").join(format!(".{a}.partial"))
    }
    pub fn list(&self) -> Result<Vec<Model>> {
        let db = self.db()?;
        let mut q = db.prepare(
            "SELECT alias,repo,preset,revision,state,desired,error,COALESCE(label,''),context_limit,EXISTS(SELECT 1 FROM model_pins p WHERE p.alias=models.alias) FROM models LEFT JOIN model_settings USING(alias) ORDER BY alias",
        )?;
        Ok(q.query_map([], |r| {
            Ok(Model {
                alias: r.get(0)?,
                repo: r.get(1)?,
                preset: r.get(2)?,
                revision: r.get(3)?,
                state: r.get(4)?,
                desired: r.get(5)?,
                error: r.get(6)?,
                label: r.get(7)?,
                context_limit: r.get(8)?,
                pinned: r.get(9)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?)
    }
    pub fn reserve(&self, a: &str, repo: &str, preset: &str) -> Result<()> {
        ensure!(
            slug(a) && valid_repo(repo) && (preset.is_empty() || slug(preset)),
            "Use a short alias, a Hugging Face model repository and a valid preset."
        );
        let db = self.db()?;
        ensure!(
            !self.package(a).exists() && !self.staging(a).exists(),
            "Package directory already exists."
        );
        ensure!(
            db.execute(
                "INSERT OR IGNORE INTO models(alias,repo,preset,state) VALUES(?,?,?,'queued')",
                params![a, repo, preset]
            )? == 1,
            "Alias already belongs to another installation."
        );
        Ok(())
    }
    pub fn settings(&self, alias: &str, label: &str, limit: Option<i32>) -> Result<()> {
        let label = label.trim();
        ensure!(
            label.chars().count() <= 64 && !label.chars().any(char::is_control),
            "Use a display label of at most 64 printable characters"
        );
        ensure!(
            limit.is_none_or(|n| n > 0),
            "Context limit must be a positive integer"
        );
        self.db()?.execute("INSERT INTO model_settings(alias,label,context_limit) VALUES(?,?,?) ON CONFLICT(alias) DO UPDATE SET label=excluded.label,context_limit=excluded.context_limit", params![alias,label,limit])?;
        Ok(())
    }
    pub fn revision(&self, a: &str, rev: &str) -> Result<()> {
        self.db()?.execute(
            "UPDATE models SET revision=? WHERE alias=?",
            params![rev, a],
        )?;
        Ok(())
    }
    pub fn finish(&self, a: &str, state: &str, error: &str) -> Result<()> {
        self.db()?.execute(
            "UPDATE models SET state=?,error=? WHERE alias=?",
            params![state, error, a],
        )?;
        Ok(())
    }
    pub fn pin(&self, alias: &str, on: bool, max: usize) -> Result<()> {
        let mut db = self.db()?;
        let tx = db.transaction()?;
        if on {
            ensure!(
                tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM models WHERE alias=? AND state='installed')",
                    [alias],
                    |r| r.get::<_, bool>(0)
                )?,
                "Install the model before pinning it"
            );
            tx.execute("INSERT OR IGNORE INTO model_pins VALUES(?)", [alias])?;
            ensure!(
                tx.query_row(
                    "SELECT COUNT(*) <= ? FROM model_pins",
                    [i64::try_from(max).unwrap_or(i64::MAX)],
                    |r| r.get::<_, bool>(0)
                )?,
                "Pin limit reached (models.max_loaded = {max}). Unpin another model first."
            );
        } else {
            tx.execute("DELETE FROM model_pins WHERE alias=?", [alias])?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn restore_order(&self, max: usize) -> Result<Vec<Model>> {
        // The file-based config can be lowered while Lum is stopped. Keep the
        // earliest pins, deterministically, rather than exceeding the new cap.
        let removed = self.db()?.execute("DELETE FROM model_pins WHERE alias IN (SELECT alias FROM model_pins ORDER BY rowid LIMIT -1 OFFSET ?)",[i64::try_from(max).unwrap_or(i64::MAX)])?;
        if removed > 0 {
            eprintln!("Released {removed} most recent pin(s) to respect models.max_loaded = {max}");
        }
        let mut models = self.list()?;
        models.retain(|m| m.state == "installed" && (m.pinned || m.desired));
        models.sort_by_key(|m| !m.pinned);
        Ok(models)
    }
    pub fn desired(&self, a: &str, on: bool) -> Result<()> {
        self.db()?.execute(
            "UPDATE models SET desired=?,error='' WHERE alias=?",
            params![on, a],
        )?;
        Ok(())
    }
    pub fn forget(&self, a: &str) -> Result<()> {
        self.db()?
            .execute("DELETE FROM models WHERE alias=?", [a])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test(flavor = "current_thread")]
    async fn blocked_sqlite_read_leaves_event_loop_available() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let lib = Library::open(tmp.path())?;
        let writer = lib.db()?;
        writer.execute_batch("BEGIN EXCLUSIVE")?;
        // If the read blocks the event loop, this rollback cannot run until
        // SQLite's busy timeout has already failed the request.
        let (read, ()) = tokio::join!(lib.run(Library::list), async {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            writer.execute_batch("ROLLBACK").unwrap();
        });
        assert!(read?.is_empty());
        Ok(())
    }
    #[test]
    fn rejects_paths_and_recovers_interrupted_download() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = Library::open(tmp.path()).unwrap();
        for bad in ["../x", "--evil", "a/b", ""] {
            assert!(!slug(bad));
        }
        assert!(!valid_repo("loom-fun/../x"));
        assert!(valid_repo("other/repo"));
        lib.reserve("chat", "loom-fun/model", "chat").unwrap();
        lib.reserve("other", "loom-fun/model", "").unwrap();
        lib.finish("other", "downloading", "").unwrap();
        let models = Library::open(tmp.path()).unwrap().list().unwrap();
        assert!(models.iter().all(|m| m.state == "failed"));
        assert!(!models[0].desired);
    }
}
