-- v0.4: backups encrypted with the Backup Master Key (age, `.zip.age`).
ALTER TABLE backups ADD COLUMN encrypted INTEGER NOT NULL DEFAULT 0;
