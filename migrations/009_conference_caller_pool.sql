CREATE TABLE IF NOT EXISTS conference_caller_rotation (
  tenant_id TEXT NOT NULL,
  conversation_id TEXT NOT NULL REFERENCES conversations(id),
  numbers_json TEXT NOT NULL,
  next_index INTEGER NOT NULL,
  PRIMARY KEY (tenant_id, conversation_id)
);
