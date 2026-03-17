CREATE TABLE project_statuses (
    id         BLOB PRIMARY KEY,
    project_id BLOB NOT NULL,
    name       TEXT NOT NULL,
    color      TEXT NOT NULL DEFAULT '#6B7280',
    sort_order INTEGER NOT NULL DEFAULT 0,
    hidden     INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now', 'subsec'))
);

CREATE INDEX idx_project_statuses_project_id ON project_statuses(project_id);

CREATE TABLE overseer_issues (
    id                BLOB PRIMARY KEY,
    project_id        BLOB NOT NULL,
    issue_number      INTEGER NOT NULL,
    simple_id         TEXT NOT NULL,
    status_id         BLOB NOT NULL,
    title             TEXT NOT NULL,
    description       TEXT,
    priority          TEXT CHECK (priority IN ('Urgent','High','Medium','Low')),
    sort_order        REAL NOT NULL DEFAULT 0,
    parent_issue_id   BLOB,
    extension_metadata TEXT NOT NULL DEFAULT '{}',
    created_at        TEXT NOT NULL DEFAULT (datetime('now', 'subsec')),
    updated_at        TEXT NOT NULL DEFAULT (datetime('now', 'subsec')),
    FOREIGN KEY (status_id) REFERENCES project_statuses(id),
    FOREIGN KEY (parent_issue_id) REFERENCES overseer_issues(id) ON DELETE SET NULL,
    UNIQUE (project_id, issue_number)
);

CREATE INDEX idx_overseer_issues_project_id ON overseer_issues(project_id);
CREATE INDEX idx_overseer_issues_status_id ON overseer_issues(status_id);
