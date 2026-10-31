-- Deleting a collection is new: the API role gets DELETE on both tables; the in_project row
-- policies are FOR ALL, so the same scoping applies.

GRANT DELETE ON collections TO colophon_api;
GRANT DELETE ON content_type_versions TO colophon_api;
