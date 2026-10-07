CREATE TABLE IF NOT EXISTS conference_browser_connections (
    connection_id TEXT PRIMARY KEY REFERENCES connections(id),
    tenant_id TEXT NOT NULL,
    conversation_id TEXT NOT NULL,
    session_id TEXT NOT NULL REFERENCES sessions(id),
    participant_id TEXT NOT NULL REFERENCES participants(id),
    request_id TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'offered'
);
