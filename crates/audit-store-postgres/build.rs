fn main() {
    // sqlx::migrate! embeds the Audit Store ledger migrations at compile time.
    // Cargo must rebuild this crate when a migration file is added.
    println!("cargo:rerun-if-changed=migrations");
}
