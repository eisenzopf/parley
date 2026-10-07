CREATE TABLE IF NOT EXISTS conference_request_evidence (
    tenant_id TEXT NOT NULL,
    subject TEXT NOT NULL,
    request_id TEXT NOT NULL,
    envelope TEXT NOT NULL,
    PRIMARY KEY (tenant_id, subject, request_id),
    FOREIGN KEY (tenant_id, subject, request_id)
      REFERENCES conference_requests(tenant_id, subject, request_id)
);
