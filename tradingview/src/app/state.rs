//! SQLite 本地状态存储：命名布局、按交易对的绘图组、通用偏好。
//!
//! 行情仍走 zip/bi5 文件；这里只持久化研究终端的 UI 状态。SQLite 连接是
//! 同步的，调用方应放在 `spawn_blocking` 里执行，避免阻塞 tokio worker。

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::data_loader::MarketType;
use crate::utils::{valid_pair, valid_timeframe};

/// 初始化表结构（幂等）。
const SCHEMA: &str = include_str!("../../migrations/001_init.sql");

/// 布局名称最大字符数。
pub const MAX_LAYOUT_NAME_CHARS: usize = 64;
/// 单个布局 payload 最大字节数。
pub const MAX_LAYOUT_PAYLOAD_BYTES: usize = 1_048_576;
/// 每个交易对最多保存的绘图数。
pub const MAX_DRAWINGS_PER_PAIR: usize = 500;
/// 单个绘图最多锚点数。
pub const MAX_DRAWING_POINTS: usize = 512;
/// 每个交易对绘图组最大字节数。
pub const MAX_DRAWINGS_PAYLOAD_BYTES: usize = 1_048_576;
/// 偏好键数量上限。
pub const MAX_PREFERENCE_KEYS: usize = 128;
/// 偏好键最大字符数。
pub const MAX_PREFERENCE_KEY_CHARS: usize = 64;
/// 单个偏好值最大字节数。
pub const MAX_PREFERENCE_VALUE_BYTES: usize = 65_536;
/// 全部偏好最大字节数。
pub const MAX_PREFERENCES_PAYLOAD_BYTES: usize = 262_144;

/// 状态存储错误。
#[derive(Debug, Error)]
pub enum StateError {
    /// 文件系统 IO 失败。
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    /// SQLite 失败。
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// JSON 序列化/反序列化失败。
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    /// 输入不合法（路由层转 400）。
    #[error("invalid state: {0}")]
    Invalid(String),
    /// 已存数据损坏或内部不变量被破坏（路由层转 500）。
    #[error("corrupt state: {0}")]
    Corrupt(String),
    /// 连接锁被 poison。
    #[error("state store lock poisoned")]
    Poisoned,
}

/// 状态存储结果。
pub type StateResult<T> = Result<T, StateError>;

/// 布局摘要（列表用，不含 payload）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayoutSummary {
    /// 布局 ID。
    pub id: String,
    /// 用户可读名称（唯一）。
    pub name: String,
    /// 市场类型。
    pub market_type: String,
    /// 交易对。
    pub pair: String,
    /// 周期。
    pub timeframe: String,
    /// 创建时间（毫秒）。
    pub created_at: i64,
    /// 最后更新时间（毫秒）。
    pub updated_at: i64,
}

/// 完整布局。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Layout {
    /// 摘要字段平铺。
    #[serde(flatten)]
    pub summary: LayoutSummary,
    /// 前端会话快照。
    pub payload: serde_json::Value,
}

/// 创建/更新布局的输入。
#[derive(Debug, Clone, Deserialize)]
pub struct LayoutInput {
    /// 名称。
    pub name: String,
    /// 市场类型。
    pub market_type: String,
    /// 交易对。
    pub pair: String,
    /// 周期。
    pub timeframe: String,
    /// 会话快照。
    pub payload: serde_json::Value,
}

/// 通用 SQLite 状态存储。
pub struct StateStore {
    conn: Mutex<Connection>,
}

impl std::fmt::Debug for StateStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StateStore").finish_non_exhaustive()
    }
}

impl StateStore {
    /// 打开（或创建）指定路径的数据库。
    ///
    /// # Errors
    /// 目录创建、SQLite 打开或建表失败时返回 [`StateError`]。
    pub fn open(path: &Path) -> StateResult<Self> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        Self::from_connection(Connection::open(path)?)
    }

    /// 内存数据库（测试用）。
    ///
    /// # Errors
    /// SQLite 初始化失败时返回 [`StateError`]。
    pub fn open_in_memory() -> StateResult<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(conn: Connection) -> StateResult<Self> {
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;",
        )?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn conn(&self) -> StateResult<MutexGuard<'_, Connection>> {
        self.conn.lock().map_err(|_| StateError::Poisoned)
    }

    /// 列出布局摘要，最近更新的在前。
    ///
    /// # Errors
    /// SQLite 查询失败时返回 [`StateError`]。
    pub fn list_layouts(&self) -> StateResult<Vec<LayoutSummary>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, name, market_type, pair, timeframe, created_at, updated_at
             FROM layouts
             ORDER BY updated_at DESC, name ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(LayoutSummary {
                id: row.get(0)?,
                name: row.get(1)?,
                market_type: row.get(2)?,
                pair: row.get(3)?,
                timeframe: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// 读取完整布局；不存在返回 `None`。
    ///
    /// # Errors
    /// SQLite 查询或 payload JSON 解析失败时返回 [`StateError`]。
    pub fn get_layout(&self, id: &str) -> StateResult<Option<Layout>> {
        let conn = self.conn()?;
        let row = conn
            .query_row(
                "SELECT id, name, market_type, pair, timeframe, created_at, updated_at, payload
                 FROM layouts WHERE id = ?1",
                params![id],
                |row| {
                    let summary = LayoutSummary {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        market_type: row.get(2)?,
                        pair: row.get(3)?,
                        timeframe: row.get(4)?,
                        created_at: row.get(5)?,
                        updated_at: row.get(6)?,
                    };
                    let payload: String = row.get(7)?;
                    Ok((summary, payload))
                },
            )
            .optional()?;
        match row {
            Some((summary, payload)) => Ok(Some(Layout {
                summary,
                payload: serde_json::from_str(&payload)?,
            })),
            None => Ok(None),
        }
    }

    /// 创建布局。
    ///
    /// # Errors
    /// 输入非法、名称重复或 SQLite 失败时返回 [`StateError`]。
    pub fn create_layout(&self, input: LayoutInput) -> StateResult<Layout> {
        validate_layout(&input)?;
        let payload = serde_json::to_string(&input.payload)?;
        let id = uuid::Uuid::new_v4().to_string();
        let now = now_ms();
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO layouts
                (id, name, market_type, pair, timeframe, payload, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                id,
                input.name.trim(),
                input.market_type,
                input.pair,
                input.timeframe,
                payload,
                now,
                now
            ],
        )
        .map_err(map_layout_constraint)?;
        drop(conn);
        self.get_layout(&id)?
            .ok_or_else(|| StateError::Corrupt("layout disappeared after insert".into()))
    }

    /// 更新布局；不存在返回 `None`。
    ///
    /// # Errors
    /// 输入非法、名称重复或 SQLite 失败时返回 [`StateError`]。
    pub fn update_layout(&self, id: &str, input: LayoutInput) -> StateResult<Option<Layout>> {
        validate_layout(&input)?;
        let payload = serde_json::to_string(&input.payload)?;
        let now = now_ms();
        let conn = self.conn()?;
        let changed = conn
            .execute(
                "UPDATE layouts
                 SET name = ?2, market_type = ?3, pair = ?4, timeframe = ?5,
                     payload = ?6, updated_at = ?7
                 WHERE id = ?1",
                params![
                    id,
                    input.name.trim(),
                    input.market_type,
                    input.pair,
                    input.timeframe,
                    payload,
                    now
                ],
            )
            .map_err(map_layout_constraint)?;
        drop(conn);
        if changed == 0 {
            Ok(None)
        } else {
            self.get_layout(id)
        }
    }

    /// 删除布局；返回是否真的删了。
    ///
    /// # Errors
    /// SQLite 失败时返回 [`StateError`]。
    pub fn delete_layout(&self, id: &str) -> StateResult<bool> {
        let conn = self.conn()?;
        let changed = conn.execute("DELETE FROM layouts WHERE id = ?1", params![id])?;
        Ok(changed > 0)
    }

    /// 读取某交易对的绘图组；没有记录时返回空数组。
    ///
    /// # Errors
    /// SQLite 失败或已存 payload 不是数组时返回 [`StateError`]。
    pub fn get_drawings(
        &self,
        market_type: &str,
        pair: &str,
    ) -> StateResult<Vec<serde_json::Value>> {
        let conn = self.conn()?;
        let payload: Option<String> = conn
            .query_row(
                "SELECT payload FROM drawings WHERE market_type = ?1 AND pair = ?2",
                params![market_type, pair],
                |row| row.get(0),
            )
            .optional()?;
        let Some(payload) = payload else {
            return Ok(Vec::new());
        };
        match serde_json::from_str::<serde_json::Value>(&payload)? {
            serde_json::Value::Array(items) => Ok(items),
            _ => Err(StateError::Corrupt(
                "stored drawings payload is not an array".into(),
            )),
        }
    }

    /// 整组替换某交易对的绘图。
    ///
    /// # Errors
    /// 输入非法或 SQLite 失败时返回 [`StateError`]。
    pub fn replace_drawings(
        &self,
        market_type: &str,
        pair: &str,
        drawings: &serde_json::Value,
    ) -> StateResult<Vec<serde_json::Value>> {
        validate_drawings(drawings)?;
        let payload = serde_json::to_string(drawings)?;
        let now = now_ms();
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO drawings (market_type, pair, payload, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(market_type, pair) DO UPDATE SET
                 payload = excluded.payload,
                 updated_at = excluded.updated_at",
            params![market_type, pair, payload, now],
        )?;
        tx.commit()?;
        drop(conn);
        self.get_drawings(market_type, pair)
    }

    /// 读取全部偏好。
    ///
    /// # Errors
    /// SQLite 或 JSON 解析失败时返回 [`StateError`]。
    pub fn get_preferences(&self) -> StateResult<serde_json::Map<String, serde_json::Value>> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare("SELECT key, value FROM preferences ORDER BY key")?;
        let rows = stmt.query_map([], |row| {
            let key: String = row.get(0)?;
            let value: String = row.get(1)?;
            Ok((key, value))
        })?;
        let mut out = serde_json::Map::new();
        for row in rows {
            let (key, value) = row?;
            out.insert(key, serde_json::from_str(&value)?);
        }
        Ok(out)
    }

    /// 合并写入偏好，返回合并后的完整集合。
    ///
    /// # Errors
    /// 输入超过限额或 SQLite 失败时返回 [`StateError`]。
    pub fn put_preferences(
        &self,
        patch: serde_json::Map<String, serde_json::Value>,
    ) -> StateResult<serde_json::Map<String, serde_json::Value>> {
        if patch.is_empty() {
            return self.get_preferences();
        }
        let mut merged = self.get_preferences()?;
        for (key, value) in &patch {
            merged.insert(key.clone(), value.clone());
        }
        validate_preferences(&merged)?;

        let now = now_ms();
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        for (key, value) in &patch {
            tx.execute(
                "INSERT INTO preferences (key, value, updated_at)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(key) DO UPDATE SET
                     value = excluded.value,
                     updated_at = excluded.updated_at",
                params![key, serde_json::to_string(value)?, now],
            )?;
        }
        tx.commit()?;
        drop(conn);
        self.get_preferences()
    }
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn map_layout_constraint(e: rusqlite::Error) -> StateError {
    if let rusqlite::Error::SqliteFailure(err, _) = &e
        && err.code == rusqlite::ErrorCode::ConstraintViolation
    {
        return StateError::Invalid("layout name already exists".into());
    }
    StateError::Sqlite(e)
}

fn validate_layout(input: &LayoutInput) -> StateResult<()> {
    let name = input.name.trim();
    if name.is_empty() || name.chars().count() > MAX_LAYOUT_NAME_CHARS {
        return Err(StateError::Invalid(format!(
            "layout name must be 1..={MAX_LAYOUT_NAME_CHARS} characters"
        )));
    }
    if input.market_type.parse::<MarketType>().is_err() {
        return Err(StateError::Invalid("invalid market_type".into()));
    }
    if !valid_pair(&input.pair) {
        return Err(StateError::Invalid("invalid pair".into()));
    }
    if !valid_timeframe(&input.timeframe) {
        return Err(StateError::Invalid("invalid timeframe".into()));
    }
    let bytes = serde_json::to_vec(&input.payload)?.len();
    if bytes > MAX_LAYOUT_PAYLOAD_BYTES {
        return Err(StateError::Invalid(format!(
            "layout payload too large (max {MAX_LAYOUT_PAYLOAD_BYTES} bytes)"
        )));
    }
    Ok(())
}

fn validate_drawings(drawings: &serde_json::Value) -> StateResult<()> {
    let items = drawings
        .as_array()
        .ok_or_else(|| StateError::Invalid("drawings must be a JSON array".into()))?;
    if items.len() > MAX_DRAWINGS_PER_PAIR {
        return Err(StateError::Invalid(format!(
            "too many drawings (max {MAX_DRAWINGS_PER_PAIR})"
        )));
    }
    for (i, item) in items.iter().enumerate() {
        let obj = item
            .as_object()
            .ok_or_else(|| StateError::Invalid(format!("drawing {i} must be an object")))?;
        if let Some(points) = obj.get("points") {
            let points = points.as_array().ok_or_else(|| {
                StateError::Invalid(format!("drawing {i} points must be an array"))
            })?;
            if points.len() > MAX_DRAWING_POINTS {
                return Err(StateError::Invalid(format!(
                    "drawing {i} has too many points (max {MAX_DRAWING_POINTS})"
                )));
            }
        }
    }
    let bytes = serde_json::to_vec(drawings)?.len();
    if bytes > MAX_DRAWINGS_PAYLOAD_BYTES {
        return Err(StateError::Invalid(format!(
            "drawings payload too large (max {MAX_DRAWINGS_PAYLOAD_BYTES} bytes)"
        )));
    }
    Ok(())
}

fn validate_preferences(prefs: &serde_json::Map<String, serde_json::Value>) -> StateResult<()> {
    if prefs.len() > MAX_PREFERENCE_KEYS {
        return Err(StateError::Invalid(format!(
            "too many preference keys (max {MAX_PREFERENCE_KEYS})"
        )));
    }
    let mut total = 0usize;
    for (key, value) in prefs {
        if key.is_empty() || key.chars().count() > MAX_PREFERENCE_KEY_CHARS {
            return Err(StateError::Invalid(format!(
                "preference key must be 1..={MAX_PREFERENCE_KEY_CHARS} characters"
            )));
        }
        let bytes = serde_json::to_vec(value)?.len();
        if bytes > MAX_PREFERENCE_VALUE_BYTES {
            return Err(StateError::Invalid(format!(
                "preference '{key}' value too large (max {MAX_PREFERENCE_VALUE_BYTES} bytes)"
            )));
        }
        total += key.len() + bytes;
    }
    if total > MAX_PREFERENCES_PAYLOAD_BYTES {
        return Err(StateError::Invalid(format!(
            "preferences payload too large (max {MAX_PREFERENCES_PAYLOAD_BYTES} bytes)"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn layout_input(name: &str) -> LayoutInput {
        LayoutInput {
            name: name.into(),
            market_type: "futures".into(),
            pair: "BTCUSDT".into(),
            timeframe: "1h".into(),
            payload: json!({"pair": "BTCUSDT", "timeframe": "1h"}),
        }
    }

    #[test]
    fn layouts_roundtrip() {
        let store = StateStore::open_in_memory().unwrap();
        let created = store.create_layout(layout_input("first")).unwrap();
        assert_eq!(created.summary.name, "first");
        assert_eq!(store.list_layouts().unwrap().len(), 1);
        assert_eq!(
            store.get_layout(&created.summary.id).unwrap(),
            Some(created.clone())
        );

        let mut input = layout_input("renamed");
        input.payload = json!({"pair": "ETHUSDT"});
        let updated = store
            .update_layout(&created.summary.id, input)
            .unwrap()
            .unwrap();
        assert_eq!(updated.summary.name, "renamed");
        assert_eq!(updated.payload, json!({"pair": "ETHUSDT"}));

        assert!(store.delete_layout(&created.summary.id).unwrap());
        assert!(store.get_layout(&created.summary.id).unwrap().is_none());
        assert!(!store.delete_layout(&created.summary.id).unwrap());
    }

    #[test]
    fn layout_name_must_be_unique() {
        let store = StateStore::open_in_memory().unwrap();
        store.create_layout(layout_input("dup")).unwrap();
        let err = store.create_layout(layout_input("dup")).unwrap_err();
        assert!(matches!(err, StateError::Invalid(_)));
    }

    #[test]
    fn layout_payload_is_limited() {
        let store = StateStore::open_in_memory().unwrap();
        let mut input = layout_input("big");
        input.payload = json!("x".repeat(MAX_LAYOUT_PAYLOAD_BYTES));
        let err = store.create_layout(input).unwrap_err();
        assert!(matches!(err, StateError::Invalid(_)));
    }

    #[test]
    fn drawings_are_replaced_per_pair() {
        let store = StateStore::open_in_memory().unwrap();
        let first = json!([{"id": "a", "points": [{"time": 1, "price": 2}]}]);
        store
            .replace_drawings("futures", "BTCUSDT", &first)
            .unwrap();
        assert_eq!(
            store.get_drawings("futures", "BTCUSDT").unwrap(),
            vec![json!({"id": "a", "points": [{"time": 1, "price": 2}]})]
        );

        let second = json!([{"id": "b"}]);
        store
            .replace_drawings("futures", "BTCUSDT", &second)
            .unwrap();
        assert_eq!(
            store.get_drawings("futures", "BTCUSDT").unwrap(),
            vec![json!({"id": "b"})]
        );
        assert!(store.get_drawings("futures", "ETHUSDT").unwrap().is_empty());
    }

    #[test]
    fn drawings_validate_shape() {
        let store = StateStore::open_in_memory().unwrap();
        assert!(matches!(
            store
                .replace_drawings("futures", "BTCUSDT", &json!({}))
                .unwrap_err(),
            StateError::Invalid(_)
        ));
        let too_many = json!(vec![json!({}); MAX_DRAWINGS_PER_PAIR + 1]);
        assert!(matches!(
            store
                .replace_drawings("futures", "BTCUSDT", &too_many)
                .unwrap_err(),
            StateError::Invalid(_)
        ));
    }

    #[test]
    fn preferences_merge_and_limit() {
        let store = StateStore::open_in_memory().unwrap();
        let mut patch = serde_json::Map::new();
        patch.insert("last_session".into(), json!({"pair": "BTCUSDT"}));
        let merged = store.put_preferences(patch).unwrap();
        assert_eq!(merged["last_session"], json!({"pair": "BTCUSDT"}));

        let mut patch = serde_json::Map::new();
        patch.insert("theme".into(), json!("dark"));
        let merged = store.put_preferences(patch).unwrap();
        assert_eq!(merged["theme"], json!("dark"));
        assert_eq!(merged["last_session"], json!({"pair": "BTCUSDT"}));
    }

    #[test]
    fn preferences_persist_across_reopen() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("state.db");
        {
            let store = StateStore::open(&path).unwrap();
            let mut patch = serde_json::Map::new();
            patch.insert("last_session".into(), json!({"pair": "BTCUSDT"}));
            store.put_preferences(patch).unwrap();
        }
        let store = StateStore::open(&path).unwrap();
        assert_eq!(
            store.get_preferences().unwrap()["last_session"],
            json!({"pair": "BTCUSDT"})
        );
    }

    #[test]
    fn invalid_layout_fields_are_rejected() {
        let store = StateStore::open_in_memory().unwrap();
        let mut input = layout_input("bad");
        input.pair = "../etc".into();
        assert!(matches!(
            store.create_layout(input).unwrap_err(),
            StateError::Invalid(_)
        ));

        let mut input = layout_input("bad");
        input.market_type = "nope".into();
        assert!(matches!(
            store.create_layout(input).unwrap_err(),
            StateError::Invalid(_)
        ));
    }
}
