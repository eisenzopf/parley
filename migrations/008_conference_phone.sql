CREATE TABLE IF NOT EXISTS conference_speaking_routes (
    session_id TEXT PRIMARY KEY REFERENCES sessions(id),
    tenant_id TEXT NOT NULL,
    connection_id TEXT NOT NULL,
    participant_id TEXT NOT NULL,
    bridge_id TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS conference_phone_moves (
    connection_id TEXT PRIMARY KEY REFERENCES connections(id),
    tenant_id TEXT NOT NULL,
    conversation_id TEXT NOT NULL,
    session_id TEXT NOT NULL REFERENCES sessions(id),
    participant_id TEXT NOT NULL REFERENCES participants(id),
    request_id TEXT NOT NULL,
    source_connection_id TEXT NOT NULL,
    source_bridge_id TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'prepared'
);
CREATE UNIQUE INDEX IF NOT EXISTS conference_phone_one_active
ON conference_phone_moves(tenant_id, session_id)
WHERE state IN ('prepared','dialing','answered','confirmed','committing','speaking','unknown','interrupted');
