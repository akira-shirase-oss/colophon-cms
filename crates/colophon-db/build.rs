//! `sqlx::migrate!` embeds `migrations/` at compile time; without this, Cargo wouldn't rebuild
//! when a migration is added or changed, and the binary would silently carry the old set.
fn main() {
    println!("cargo:rerun-if-changed=migrations");
}
