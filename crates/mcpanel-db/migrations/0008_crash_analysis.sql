-- v0.4: crash analysis (root-cause exception, suspected plugins/mods).
ALTER TABLE crash_events ADD COLUMN analysis_json TEXT NOT NULL DEFAULT '{}';
