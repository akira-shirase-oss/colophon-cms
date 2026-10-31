-- Deleting an entry is new: until now nothing ever deleted content. The API role gets DELETE
-- on both tables; the in_project row policies are FOR ALL, so the same scoping applies.

GRANT DELETE ON entries TO colophon_api;
GRANT DELETE ON revisions TO colophon_api;
