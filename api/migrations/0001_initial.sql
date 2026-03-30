CREATE TABLE clients (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    host TEXT NOT NULL,
    color TEXT NOT NULL DEFAULT '#6366f1',
    compose_file_path TEXT,
    agent_version TEXT,
    agent_update_mode TEXT NOT NULL DEFAULT 'manual',
    last_seen TEXT,
    connected INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL
);

CREATE TABLE containers (
    id TEXT PRIMARY KEY,
    client_id TEXT NOT NULL REFERENCES clients(id) ON DELETE CASCADE,
    container_name TEXT NOT NULL,
    image TEXT NOT NULL,
    current_digest TEXT,
    latest_digest TEXT,
    update_available INTEGER NOT NULL DEFAULT 0,
    update_mode TEXT NOT NULL DEFAULT 'manual',
    status TEXT NOT NULL DEFAULT 'running',
    checked_at TEXT,
    UNIQUE(client_id, container_name)
);

CREATE TABLE update_jobs (
    id TEXT PRIMARY KEY,
    client_id TEXT NOT NULL REFERENCES clients(id) ON DELETE CASCADE,
    container_name TEXT NOT NULL,
    image TEXT NOT NULL,
    from_digest TEXT,
    to_digest TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    output TEXT,
    started_at TEXT NOT NULL,
    completed_at TEXT
);

CREATE INDEX idx_containers_client ON containers(client_id);
CREATE INDEX idx_jobs_client ON update_jobs(client_id);
CREATE INDEX idx_jobs_started ON update_jobs(started_at);
