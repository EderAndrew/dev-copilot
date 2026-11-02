-- Schema v1 (specs/001-meeting-sessions/data-model.md).
-- Timestamps and offsets are INTEGER milliseconds (UTC epoch / from session start).
-- Ids are hyphenated UUID v7 strings. Deleting a session cascades to all of its content.

CREATE TABLE sessions (
    id                  TEXT PRIMARY KEY NOT NULL,
    title               TEXT NOT NULL,
    language            TEXT NOT NULL,
    state               TEXT NOT NULL CHECK (state IN
                            ('idle', 'starting', 'capturing', 'paused', 'stopping', 'ended', 'failed')),
    failure             TEXT,
    created_at          INTEGER NOT NULL,
    started_at          INTEGER,
    ended_at            INTEGER,
    active_duration_ms  INTEGER NOT NULL DEFAULT 0 CHECK (active_duration_ms >= 0),
    CHECK ((state = 'failed') = (failure IS NOT NULL)),
    CHECK (ended_at IS NULL OR (started_at IS NOT NULL AND ended_at >= started_at))
) STRICT;

CREATE INDEX sessions_created_at ON sessions (created_at DESC, id);

CREATE TABLE session_sources (
    session_id  TEXT NOT NULL REFERENCES sessions (id) ON DELETE CASCADE,
    kind        TEXT NOT NULL CHECK (kind IN ('microphone', 'system')),
    device_id   TEXT,
    PRIMARY KEY (session_id, kind)
) STRICT;

CREATE TABLE segments (
    id          TEXT PRIMARY KEY NOT NULL,
    session_id  TEXT NOT NULL REFERENCES sessions (id) ON DELETE CASCADE,
    seq         INTEGER NOT NULL CHECK (seq >= 0),
    kind        TEXT NOT NULL CHECK (kind IN ('final', 'gap')),
    text        TEXT,
    gap_reason  TEXT,
    source      TEXT CHECK (source IS NULL OR source IN ('microphone', 'system')),
    start_ms    INTEGER NOT NULL CHECK (start_ms >= 0),
    end_ms      INTEGER NOT NULL,
    UNIQUE (session_id, seq),
    CHECK (end_ms >= start_ms),
    CHECK ((kind = 'final' AND text IS NOT NULL AND gap_reason IS NULL)
        OR (kind = 'gap' AND text IS NULL AND gap_reason IS NOT NULL))
) STRICT;

CREATE TABLE analysis_runs (
    id             TEXT PRIMARY KEY NOT NULL,
    session_id     TEXT NOT NULL REFERENCES sessions (id) ON DELETE CASCADE,
    provider_id    TEXT NOT NULL,
    trigger        TEXT NOT NULL CHECK (trigger IN ('incremental', 'final')),
    seq_from       INTEGER NOT NULL,
    seq_to         INTEGER NOT NULL CHECK (seq_to >= seq_from),
    outcome        TEXT CHECK (outcome IS NULL OR outcome IN
                       ('ok', 'invalid_output', 'provider_error', 'rate_limited')),
    created_count  INTEGER NOT NULL DEFAULT 0,
    started_at     INTEGER NOT NULL,
    finished_at    INTEGER
) STRICT;

CREATE INDEX analysis_runs_session ON analysis_runs (session_id);

CREATE TABLE records (
    id                      TEXT PRIMARY KEY NOT NULL,
    session_id              TEXT NOT NULL REFERENCES sessions (id) ON DELETE CASCADE,
    kind                    TEXT NOT NULL CHECK (kind IN
                                ('requirement', 'decision', 'risk', 'open_question', 'task', 'assumption')),
    text                    TEXT NOT NULL,
    nature                  TEXT NOT NULL CHECK (nature IN ('observed', 'inferred', 'suggested')),
    status                  TEXT NOT NULL CHECK (status IN ('proposed', 'confirmed', 'rejected', 'pending')),
    rationale               TEXT NOT NULL DEFAULT '',
    origin                  TEXT NOT NULL CHECK (origin IN ('ai', 'user')),
    origin_provider_id      TEXT,
    origin_analysis_run_id  TEXT REFERENCES analysis_runs (id) ON DELETE SET NULL,
    acceptance_criteria     TEXT NOT NULL DEFAULT '[]',
    alternatives            TEXT NOT NULL DEFAULT '[]',
    conflicts_with          TEXT REFERENCES records (id) ON DELETE SET NULL,
    duplicate_of            TEXT REFERENCES records (id) ON DELETE SET NULL,
    created_at              INTEGER NOT NULL,
    updated_at              INTEGER NOT NULL,
    CHECK (origin = 'user' OR origin_provider_id IS NOT NULL)
) STRICT;

CREATE INDEX records_session ON records (session_id);

CREATE TABLE record_evidence (
    record_id   TEXT NOT NULL REFERENCES records (id) ON DELETE CASCADE,
    segment_id  TEXT NOT NULL REFERENCES segments (id) ON DELETE CASCADE,
    PRIMARY KEY (record_id, segment_id)
) STRICT;

CREATE TABLE record_changes (
    id                     TEXT PRIMARY KEY NOT NULL,
    record_id              TEXT NOT NULL REFERENCES records (id) ON DELETE CASCADE,
    actor                  TEXT NOT NULL CHECK (actor IN ('user', 'ai')),
    actor_analysis_run_id  TEXT,
    field                  TEXT NOT NULL CHECK (field IN
                               ('created', 'text', 'status', 'rationale', 'acceptance_criteria', 'alternatives')),
    old_value              TEXT,
    new_value              TEXT,
    at                     INTEGER NOT NULL,
    CHECK (actor = 'user' OR field = 'created')
) STRICT;

CREATE INDEX record_changes_record ON record_changes (record_id, at);

CREATE TABLE questions (
    id           TEXT PRIMARY KEY NOT NULL,
    session_id   TEXT NOT NULL REFERENCES sessions (id) ON DELETE CASCADE,
    question     TEXT NOT NULL,
    answer       TEXT,
    certainty    TEXT CHECK (certainty IS NULL OR certainty IN ('supported', 'partial', 'no_evidence')),
    status       TEXT NOT NULL CHECK (status IN ('pending', 'answered', 'failed')),
    provider_id  TEXT,
    asked_at     INTEGER NOT NULL,
    answered_at  INTEGER
) STRICT;

CREATE INDEX questions_session ON questions (session_id, asked_at);

CREATE TABLE question_evidence (
    question_id  TEXT NOT NULL REFERENCES questions (id) ON DELETE CASCADE,
    segment_id   TEXT REFERENCES segments (id) ON DELETE CASCADE,
    record_id    TEXT REFERENCES records (id) ON DELETE CASCADE,
    CHECK ((segment_id IS NULL) <> (record_id IS NULL)),
    UNIQUE (question_id, segment_id, record_id)
) STRICT;

CREATE TABLE providers (
    id               TEXT PRIMARY KEY NOT NULL,
    role             TEXT NOT NULL CHECK (role IN ('transcription', 'analysis')),
    adapter          TEXT NOT NULL,
    endpoint         TEXT,
    model            TEXT NOT NULL,
    credential_ref   TEXT,
    data_categories  TEXT NOT NULL
) STRICT;

CREATE TABLE provider_authorizations (
    id                INTEGER PRIMARY KEY,
    provider_id       TEXT NOT NULL REFERENCES providers (id) ON DELETE CASCADE,
    state             TEXT NOT NULL CHECK (state IN ('granted', 'revoked')),
    destination       TEXT NOT NULL,
    data_categories   TEXT NOT NULL,
    retention_notice  TEXT NOT NULL,
    granted_at        INTEGER NOT NULL,
    revoked_at        INTEGER
) STRICT;

CREATE INDEX provider_authorizations_provider ON provider_authorizations (provider_id);

CREATE TABLE exports (
    id          TEXT PRIMARY KEY NOT NULL,
    session_id  TEXT NOT NULL REFERENCES sessions (id) ON DELETE CASCADE,
    formats     TEXT NOT NULL,
    content     TEXT NOT NULL,
    at          INTEGER NOT NULL
) STRICT;

CREATE TABLE settings (
    key    TEXT PRIMARY KEY NOT NULL,
    value  TEXT NOT NULL
) STRICT;
