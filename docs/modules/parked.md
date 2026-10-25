# Parked and later

> What is deliberately out of v0.1, why, and what has to be true before it comes back.

These areas are fully specified in the master spec. They are out of v0.1 because each adds a large product surface and
breadth rather than proof (D-006). "Parked" areas return only by an explicit decision. "Later" areas are expected
after v0.1 once the core is proven.

**Re-entry rule for everything below:** v0.1's definition of done ([END_GOAL.md](../END_GOAL.md)) is met, the
correctness stop conditions are clear, and the maintainer records the decision in [DECISIONS.md](../DECISIONS.md).

## Parked

### Commerce catalog, carts and checkout (spec §20)
- **What the spec requires:**
  - one merchant per store, with one Stripe account per environment;
  - products and variants with immutable price records stored as integer minor units;
  - server-authoritative totals;
  - Stripe-hosted Checkout created by a worker with a stable operation key.
- **Why parked:** it is a second product with its own provider, legal and fraud surface.
- **Re-entry:** a provider simulator for tests, and an independent security review before any production commerce
  (spec §37).

### Inventory, shipping and tax (spec §21)
- **What the spec requires:**
  - an append-only inventory ledger with `on_hand`, `reserved` and `allocated`;
  - sorted-lock reservations;
  - `paid_unfulfillable` for late payments;
  - configured shipping rates;
  - Stripe Tax or merchant-approved rates. Tax is never inferred by a model.
- **Why parked:** it depends on commerce, and needs merchant-specific configuration and review.
- **Re-entry:** after commerce; requires merchant region configuration and review.

### Payments, refunds and reconciliation (spec §22)
- **What the spec requires:**
  - verified raw-body webhook signatures and a durable event inbox;
  - reconciliation every 15 minutes for recent pending operations;
  - refund capacity reserved in a transaction;
  - dispute tracking.
- **Why parked:** it depends on commerce.
- **Re-entry:** together with commerce. Never load-test Stripe or run real payments as a benchmark.

### Subscriptions, memberships and private delivery (spec §23)
- **What the spec requires:**
  - fixed recurring plans;
  - entitlements up to the recorded paid-through time, with a configurable grace period (default zero);
  - the customer portal through Rust-issued sessions;
  - private content kept out of every shared cache.
- **Why parked:** it depends on commerce and payments.
- **Re-entry:** after payments are proven.

### Hosting adapters and custom domains (spec §25)
- **What the spec requires:**
  - a GitHub app with a branch/PR policy;
  - a tested Vercel adapter;
  - domain ownership proof and asynchronous certificate provisioning;
  - cache namespaces and TLS routing changed together.
- **Why parked:** v0.1 needs one reference deployment path, not a hosting platform.
- **Re-entry:** when a second deployment target is genuinely needed.

### Membership and knowledge-base templates (spec §19)
- **What the spec requires:** membership with gated content (needs subscriptions); a knowledge base with versions,
  search and private articles.
- **Why parked:** one excellent blog template first (D-009).
- **Re-entry:** the knowledge base after v0.1 if there is a real user for it; membership only after subscriptions.

## Later (expected after v0.1)

### Forms, leads and notifications (spec §24)
- **What the spec requires:**
  - a lead and its notification outbox are created in one transaction;
  - submission keys prevent duplicates;
  - consent text is recorded;
  - abuse controls;
  - at most five delivery attempts within 24 hours.
- **Re-entry:** together with a business-site template, reusing the outbox and worker from P2.

### Source import adapters (spec §12)
- **What the spec requires:** normalized Strapi, Sanity and Payload export manifests mapped into a dry-run plan.
  Incompatible fields are reported, never dropped.
- **Re-entry:** native JSONL import (P5) is proven first.

### Schema designer for non-technical users (spec §12)
- **What the spec requires:** the UI proposes safe field additions with example data. Developer-controlled collections
  stay read-only. There is one canonical schema registry shared with the CLI.
- **Re-entry:** once schema migrations (P6) are stable.

### Full localization UI and translation workflow (spec §06)
- **What the spec requires:** per-locale drafts, publication history and validation, explicit fallback policy, and
  translation proposals as locale revisions.
- **Note:** the locale is in the data model from P1. Only the full editor workflow waits.

## A possible showcase

If commerce ever returns, the first piece should be the spec's X01 case as a standalone demo: **100 simultaneous
checkouts for the last unit, and exactly one wins**. It is verified against SQL state, not HTTP codes, using a
simulated payment provider. It shows the same correctness engine applied to money, without building a whole store.
