-- P7, D-142: visitor auth events belong in the audit log staff already read, but
-- `audit_events.actor_id` references `identities` and a site user is not an identity — so the
-- events name the shared site-auth system identity as actor (the same pattern the worker's
-- `budget.discrepancy` and the restorer's rewrites use), and the subject carries the site
-- user's id or the email a sign-in link was sent to. Nothing can sign in as this identity:
-- `urn:colophon:system` is no OIDC issuer and the row has no password.
INSERT INTO identities (id, issuer, subject, display_name)
VALUES ('a1b2c3d4-0000-4000-8000-0000000000a1', 'urn:colophon:system', 'audience-auth', 'Site sign-in')
ON CONFLICT (id) DO NOTHING;
