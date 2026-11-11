-- 研究终端本地状态库（SQLite）。
--
-- 只存 UI/研究状态，不存行情：
--   layouts     命名布局（完整快照 JSON）
--   drawings    按 (market_type, pair) 的绘图组，跨周期共享；payload 是
--               lightweight-charts-drawing 的 JSON 数组
--   preferences 通用键值偏好（last_session 等）

CREATE TABLE IF NOT EXISTS layouts (
    id          TEXT PRIMARY KEY NOT NULL,
    name        TEXT NOT NULL UNIQUE,
    market_type TEXT NOT NULL,
    pair        TEXT NOT NULL,
    timeframe   TEXT NOT NULL,
    payload     TEXT NOT NULL,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS layouts_updated_at_idx
    ON layouts (updated_at DESC);

CREATE TABLE IF NOT EXISTS drawings (
    market_type TEXT NOT NULL,
    pair        TEXT NOT NULL,
    payload     TEXT NOT NULL,
    updated_at  INTEGER NOT NULL,
    PRIMARY KEY (market_type, pair)
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS preferences (
    key        TEXT PRIMARY KEY NOT NULL,
    value      TEXT NOT NULL,
    updated_at INTEGER NOT NULL
) WITHOUT ROWID;
