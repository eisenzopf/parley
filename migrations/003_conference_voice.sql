CREATE TABLE IF NOT EXISTS conference_voice_operations (
    tenant_id TEXT NOT NULL,
    conversation_id TEXT NOT NULL,
    session_id TEXT PRIMARY KEY REFERENCES sessions(id),
    request_id TEXT NOT NULL,
    target_participant_id TEXT NOT NULL REFERENCES participants(id),
    assistant_participant_id TEXT NOT NULL REFERENCES participants(id),
    remote_connection_id TEXT NOT NULL UNIQUE,
    purpose TEXT NOT NULL,
    ai_state TEXT NOT NULL DEFAULT 'waiting',
    ai_connection_id TEXT,
    bridge_id TEXT
);
