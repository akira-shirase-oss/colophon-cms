-- Milestone M, step M.3: structures. docs/modules/02-db.md, D-066.
--
-- A reference can sit inside an object, a list or a page builder's block, so where it is in the
-- content is a path, not just a field id: `author`, `sections/2/by`, `people/0`. Its parts are
-- field ids and list positions, at most four structures deep (MAX_DEPTH plus the field itself).
ALTER TABLE revision_references DROP CONSTRAINT revision_references_field_check;
ALTER TABLE revision_references ADD CONSTRAINT revision_references_field_check CHECK (
    char_length(field) <= 400
    AND field ~ '^[a-z][a-z0-9_]{0,63}(/([0-9]{1,2}|[a-z][a-z0-9_]{0,63})){0,8}$'
);
