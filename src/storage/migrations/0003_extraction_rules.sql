-- Learned or caller-supplied CSS extraction rules, per host and page template.
CREATE TABLE IF NOT EXISTS extraction_rules (
    host TEXT NOT NULL,
    template_id TEXT NOT NULL,
    field TEXT NOT NULL,
    selector TEXT NOT NULL,
    value_type TEXT NOT NULL DEFAULT 'text',
    source TEXT NOT NULL DEFAULT 'learned',
    support INTEGER NOT NULL DEFAULT 1,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (host, template_id, field)
);
