-- Additive conference profile storage; legacy conversations/messages remain canonical.
CREATE TABLE IF NOT EXISTS conference_members (
  tenant_id TEXT NOT NULL,
  conversation_id TEXT NOT NULL REFERENCES conversations(id),
  participant_id TEXT NOT NULL REFERENCES participants(id),
  subject TEXT NOT NULL,
  alias TEXT NOT NULL,
  role TEXT NOT NULL,
  PRIMARY KEY (tenant_id, conversation_id, participant_id),
  UNIQUE (tenant_id, conversation_id, subject),
  UNIQUE (tenant_id, conversation_id, alias)
);
CREATE TABLE IF NOT EXISTS participant_endpoints (
  tenant_id TEXT NOT NULL,
  participant_id TEXT NOT NULL REFERENCES participants(id),
  kind TEXT NOT NULL,
  address TEXT NOT NULL,
  PRIMARY KEY (tenant_id, participant_id, kind)
);
CREATE TABLE IF NOT EXISTS conference_tokens (
  token_hash TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL,
  subject TEXT NOT NULL,
  expires_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS conference_requests (
  tenant_id TEXT NOT NULL,
  subject TEXT NOT NULL,
  request_id TEXT NOT NULL,
  fingerprint TEXT NOT NULL,
  response TEXT,
  created_at TEXT NOT NULL,
  PRIMARY KEY (tenant_id, subject, request_id)
);
CREATE TABLE IF NOT EXISTS message_recipients (
  tenant_id TEXT NOT NULL,
  message_id TEXT NOT NULL REFERENCES messages(id),
  participant_id TEXT NOT NULL REFERENCES participants(id),
  PRIMARY KEY (tenant_id, message_id, participant_id)
);
CREATE TABLE IF NOT EXISTS message_metadata (
  message_id TEXT PRIMARY KEY REFERENCES messages(id),
  content_type TEXT NOT NULL,
  in_reply_to TEXT,
  request_id TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS message_deliveries (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL,
  conversation_id TEXT NOT NULL REFERENCES conversations(id),
  message_id TEXT NOT NULL REFERENCES messages(id),
  participant_id TEXT NOT NULL REFERENCES participants(id),
  sender_address TEXT NOT NULL,
  recipient_address TEXT NOT NULL,
  provider_id TEXT,
  state TEXT NOT NULL,
  error TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE (tenant_id, message_id, participant_id),
  UNIQUE (tenant_id, provider_id)
);
CREATE TABLE IF NOT EXISTS conference_events (
  seq INTEGER PRIMARY KEY AUTOINCREMENT,
  tenant_id TEXT NOT NULL,
  conversation_id TEXT NOT NULL REFERENCES conversations(id),
  event_type TEXT NOT NULL,
  request_id TEXT,
  audience TEXT NOT NULL,
  payload TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS conference_inbound_sms (
  tenant_id TEXT NOT NULL,
  provider_id TEXT NOT NULL,
  message_id TEXT NOT NULL REFERENCES messages(id),
  PRIMARY KEY (tenant_id, provider_id)
);
CREATE INDEX IF NOT EXISTS idx_conference_events_cursor ON conference_events(tenant_id, conversation_id, seq);
CREATE INDEX IF NOT EXISTS idx_delivery_outbox ON message_deliveries(state, created_at);
CREATE INDEX IF NOT EXISTS idx_delivery_reply_route ON message_deliveries(tenant_id, sender_address, recipient_address);
