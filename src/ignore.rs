use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::model::Issue;
use crate::paths;

pub const IGNORE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IgnoreEntry {
    pub id: String,
    pub rule_id: String,
    pub original: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IgnoreStore {
    pub schema_version: u32,
    #[serde(default)]
    pub entries: Vec<IgnoreEntry>,
    #[serde(skip)]
    path: PathBuf,
}

impl Default for IgnoreStore {
    fn default() -> Self {
        Self {
            schema_version: IGNORE_SCHEMA_VERSION,
            entries: Vec::new(),
            path: PathBuf::new(),
        }
    }
}

impl IgnoreStore {
    pub fn load_default() -> Result<Self> {
        Self::load(&paths::ignores_path()?)
    }

    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self {
                schema_version: IGNORE_SCHEMA_VERSION,
                entries: Vec::new(),
                path: path.to_owned(),
            });
        }
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("无法读取忽略库 {}", path.display()))?;
        let mut store: Self = serde_json::from_str(&text)
            .with_context(|| format!("忽略库格式错误 {}", path.display()))?;
        if store.schema_version != IGNORE_SCHEMA_VERSION {
            anyhow::bail!(
                "不支持的忽略库版本 {}（当前支持 {}）",
                store.schema_version,
                IGNORE_SCHEMA_VERSION
            );
        }
        store.path = path.to_owned();
        Ok(store)
    }

    pub fn save(&self) -> Result<()> {
        if self.path.as_os_str().is_empty() {
            anyhow::bail!("忽略库路径为空");
        }
        paths::ensure_parent(&self.path)?;
        let json = serde_json::to_string_pretty(self)?;
        let temporary = self.path.with_extension("json.tmp");
        std::fs::write(&temporary, format!("{json}\n"))
            .with_context(|| format!("无法写入临时忽略库 {}", temporary.display()))?;
        std::fs::rename(&temporary, &self.path)
            .with_context(|| format!("无法更新忽略库 {}", self.path.display()))?;
        Ok(())
    }

    pub fn add(
        &mut self,
        rule_id: &str,
        original: &str,
        note: Option<String>,
    ) -> Result<IgnoreEntry> {
        if rule_id.trim().is_empty() || original.is_empty() {
            anyhow::bail!("规则 ID 与原文不能为空");
        }
        if let Some(existing) = self
            .entries
            .iter()
            .find(|entry| entry.rule_id == rule_id && entry.original == original)
        {
            return Ok(existing.clone());
        }
        let entry = IgnoreEntry {
            id: next_id(rule_id, original, self.entries.len()),
            rule_id: rule_id.to_owned(),
            original: original.to_owned(),
            note: note.filter(|note| !note.trim().is_empty()),
            created_at: chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
        };
        self.entries.push(entry.clone());
        self.save()?;
        Ok(entry)
    }

    pub fn remove(&mut self, id: &str) -> Result<Option<IgnoreEntry>> {
        let Some(index) = self.entries.iter().position(|entry| entry.id == id) else {
            return Ok(None);
        };
        let removed = self.entries.remove(index);
        self.save()?;
        Ok(Some(removed))
    }

    pub fn matches(&self, issue: &Issue) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.rule_id == issue.rule_id && entry.original == issue.original)
    }

    pub fn fingerprints(&self) -> HashSet<(String, String)> {
        self.entries
            .iter()
            .map(|entry| (entry.rule_id.clone(), entry.original.clone()))
            .collect()
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

pub fn match_fingerprint(rule_id: &str, original: &str) -> HashSet<(String, String)> {
    let mut set = HashSet::new();
    set.insert((rule_id.to_owned(), original.to_owned()));
    set
}

fn next_id(rule_id: &str, original: &str, ordinal: usize) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    rule_id.hash(&mut hasher);
    original.hash(&mut hasher);
    nanos.hash(&mut hasher);
    ordinal.hash(&mut hasher);
    std::process::id().hash(&mut hasher);
    format!("ig-{:012x}", hasher.finish() & 0xffff_ffff_ffff)
}
