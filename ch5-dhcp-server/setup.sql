PRAGMA foreign_keys=OFF;
BEGIN TRANSACTION;
CREATE TABLE IF NOT EXISTS "lease_entries"
(
"id" integer primary key autoincrement,
"mac_addr" text not null unique,
"ip_addr" text not null,
"deleted" unsigned integer not null default 0
);
DELETE FROM sqlite_sequence;
COMMIT;
