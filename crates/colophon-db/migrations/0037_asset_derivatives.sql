-- Renditions the worker proved it wrote: the names a `/media/v1/{…}/{asset}/{name}` request may
-- answer for. The worker alone writes them (M.3, D-096).
ALTER TABLE assets ADD COLUMN derivatives text[] NOT NULL DEFAULT '{}';

-- The worker's update was granted column by column in 0035; the new column joins it.
GRANT UPDATE (derivatives) ON assets TO colophon_worker;
