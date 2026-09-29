-- v0.4: optional size cap for a server's scheduled backups (GiB; 0 = no cap).
ALTER TABLE backup_policies ADD COLUMN max_total_gb INTEGER NOT NULL DEFAULT 0;
