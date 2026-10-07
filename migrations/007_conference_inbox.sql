-- Ambiguous inbound text is private until an administrator resolves its route.
CREATE TABLE IF NOT EXISTS conference_held_sms (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    tenant_id TEXT NOT NULL,
    provider_id TEXT NOT NULL,
    remote_address TEXT NOT NULL,
    local_address TEXT NOT NULL,
    body TEXT NOT NULL,
    candidates TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'held',
    received_at TEXT NOT NULL,
    message_id TEXT REFERENCES messages(id),
    resolved_conversation_id TEXT,
    resolved_participant_id TEXT,
    resolved_by TEXT,
    resolution_note TEXT,
    resolved_at TEXT,
    UNIQUE (tenant_id, provider_id)
);
CREATE INDEX IF NOT EXISTS idx_conference_held_sms ON conference_held_sms(tenant_id, state, id);
