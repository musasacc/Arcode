-- The 1M-row test database (docs/PLUGINS.md §13, Step 11). Run by the
-- plugin's native test (tests/big.rs) and, through the plugin's own Query
-- tab with writes on, by the host's end-to-end test. Row i (0-based) of
-- `dense` has id i + 1; of `gaps`, id 3 * (i + 1). Both labels are
-- 'row <i + 1>'.
CREATE TABLE dense(id INTEGER PRIMARY KEY, k INTEGER NOT NULL, label TEXT NOT NULL);
CREATE TABLE gaps(id INTEGER PRIMARY KEY, k INTEGER NOT NULL, label TEXT NOT NULL);
WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 1000000)
INSERT INTO dense(id, k, label) SELECT i, i % 1000, 'row ' || i FROM n;
INSERT INTO gaps(id, k, label) SELECT id * 3, k, label FROM dense;
