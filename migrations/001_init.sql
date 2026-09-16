PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS tenants (
    id TEXT PRIMARY KEY,
    api_secret_hash TEXT NOT NULL,
    created_at TEXT NOT NULL,
    config TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE IF NOT EXISTS identities (
    tenant_id TEXT NOT NULL,
    key_type TEXT NOT NULL,
    key_value TEXT NOT NULL,
    conversation_id TEXT NOT NULL,
    do_not_reopen INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (tenant_id, key_type, key_value)
);

CREATE TABLE IF NOT EXISTS conversations (
    id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    state TEXT NOT NULL,
    policy TEXT NOT NULL,
    opened_at TEXT NOT NULL,
    closed_at TEXT,
    last_activity_at TEXT NOT NULL,
    vapi_chat_session_id TEXT,
    metadata TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE IF NOT EXISTS participants (
    id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    conversation_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    role TEXT NOT NULL,
    identity_ref TEXT,
    display_name TEXT,
    joined_at TEXT NOT NULL,
    left_at TEXT
);

CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    conversation_id TEXT NOT NULL,
    medium TEXT NOT NULL,
    state TEXT NOT NULL,
    started_at TEXT NOT NULL,
    ended_at TEXT
);

CREATE TABLE IF NOT EXISTS connections (
    id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    participant_id TEXT NOT NULL,
    transport TEXT NOT NULL,
    state TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS messages (
    id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    conversation_id TEXT NOT NULL,
    from_participant TEXT,
    medium TEXT NOT NULL,
    body TEXT NOT NULL,
    provider_id TEXT,
    state TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS events (
    id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    conversation_id TEXT,
    type TEXT NOT NULL,
    payload TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS vcons (
    id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    conversation_id TEXT,
    session_id TEXT,
    path TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS operators (
    id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    email TEXT NOT NULL,
    password_hash TEXT,
    created_at TEXT NOT NULL,
    UNIQUE (tenant_id, email)
);

CREATE TABLE IF NOT EXISTS operator_sessions (
    token_hash TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    operator_id TEXT NOT NULL,
    expires_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS idempotency (
    tenant_id TEXT NOT NULL,
    key TEXT NOT NULL,
    resource_id TEXT NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (tenant_id, key)
);

CREATE INDEX IF NOT EXISTS idx_identities_conversation
    ON identities (tenant_id, conversation_id);
CREATE INDEX IF NOT EXISTS idx_messages_conversation
    ON messages (tenant_id, conversation_id, created_at);
CREATE INDEX IF NOT EXISTS idx_events_conversation
    ON events (tenant_id, conversation_id, created_at);
CREATE INDEX IF NOT EXISTS idx_sessions_conversation
    ON sessions (tenant_id, conversation_id, started_at);
CREATE INDEX IF NOT EXISTS idx_participants_conversation
    ON participants (tenant_id, conversation_id);
