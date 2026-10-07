-- A signed provider receipt may arrive before the send response is persisted.
CREATE TABLE IF NOT EXISTS conference_provider_receipts (
    tenant_id TEXT NOT NULL,
    provider_id TEXT NOT NULL,
    state TEXT NOT NULL,
    error TEXT,
    PRIMARY KEY (tenant_id, provider_id)
);
