//! Whose stack is this? (D-187) The tests that stop and start a stack service call
//! [`require_own_stack`] first, under the same rule as `scripts/stack-guard.sh`: the stack
//! `deploy/compose.yaml` reaches (project `colophon`) is this run's own only in CI, under a
//! `COMPOSE_PROJECT_NAME` other than `colophon`, or when its owner sets `COLOPHON_STACK_OWNER=1`.
//! On anyone else's stack the test fails before stopping anything.

/// The rule, from the three variables' values.
pub(crate) fn stack_is_ours(ci: Option<&str>, project: Option<&str>, owner: Option<&str>) -> bool {
    ci.is_some_and(|value| !value.is_empty())
        || project.is_some_and(|name| !name.is_empty() && name != "colophon")
        || owner == Some("1")
}

/// Panics, naming what was refused, unless the stack is this run's own.
pub(crate) fn require_own_stack(what: &str) {
    let var = |name: &str| std::env::var(name).ok();
    let ours = stack_is_ours(
        var("CI").as_deref(),
        var("COMPOSE_PROJECT_NAME").as_deref(),
        var("COLOPHON_STACK_OWNER").as_deref(),
    );
    assert!(
        ours,
        "refusing to {what} on the shared 'colophon' stack (D-187): run this test in CI, on a stack \
         of your own (COMPOSE_PROJECT_NAME=<name>), or set COLOPHON_STACK_OWNER=1 if the stack is yours"
    );
}
