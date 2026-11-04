-- Milestone P5: the provider-budget ceilings (spec §17, §547; D-122, invariant AU3).
--
-- `budget_reservations` has carried the `tokens` and `cost` kinds since migration 0045; what
-- was missing is the columns that bound them. `api_token_limits` gains the provider half of a
-- token's ceilings — a day of provider tokens and a day of provider cost — so `reserve` can
-- refuse a call that would spend past them instead of admitting it unconditionally.

-- Provider tokens the token's requests may consume per rolling day: the assistant's prompts
-- and completions, counted as the provider bills them (§547's per-request token allowance).
ALTER TABLE api_token_limits
    ADD COLUMN tokens_per_day bigint CHECK (tokens_per_day BETWEEN 1 AND 1000000000);
-- Provider cost per rolling day, in micros of the accounting unit — the unit's millionths
-- (1/1,000,000; USD micros for a dollar-denominated provider), a fixed-point integer so a
-- limit is never a float. What a call may spend, settled with what the outcome consumed.
ALTER TABLE api_token_limits
    ADD COLUMN cost_per_day_micros bigint CHECK (cost_per_day_micros BETWEEN 1 AND 100000000000);

-- The row must still bound something; the unnamed CHECK from migration 0045 is replaced to
-- count the provider columns too.
ALTER TABLE api_token_limits DROP CONSTRAINT api_token_limits_check;
ALTER TABLE api_token_limits ADD CONSTRAINT api_token_limits_bounds_something
    CHECK (num_nonnulls(documents_per_day, max_concurrent_jobs, requests_per_minute,
                        tokens_per_day, cost_per_day_micros) >= 1);
