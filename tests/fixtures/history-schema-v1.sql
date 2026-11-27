CREATE TABLE clips (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    capture_id TEXT NOT NULL UNIQUE,
    captured_at_ms INTEGER NOT NULL CHECK(captured_at_ms >= 0),
    text_full TEXT,
    preview TEXT NOT NULL,
    source_process TEXT,
    source_confidence TEXT NOT NULL CHECK(source_confidence IN ('owner','unknown')),
    omitted_formats_json TEXT NOT NULL,
    payload_bytes INTEGER NOT NULL CHECK(payload_bytes >= 0)
);
CREATE TABLE payloads (
    clip_id INTEGER NOT NULL REFERENCES clips(id) ON DELETE CASCADE,
    format_name TEXT NOT NULL CHECK(format_name IN ('CF_UNICODETEXT','HTML Format','PNG')),
    encoding_version INTEGER NOT NULL CHECK(encoding_version = 1),
    byte_size INTEGER NOT NULL CHECK(byte_size >= 0 AND byte_size = length(data)),
    data BLOB NOT NULL,
    PRIMARY KEY(clip_id,format_name)
);
CREATE INDEX clips_recent ON clips(captured_at_ms DESC,id DESC);
PRAGMA application_id=0x504E4631;
PRAGMA user_version=1;
