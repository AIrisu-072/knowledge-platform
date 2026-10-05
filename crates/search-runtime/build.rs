fn main() {
    // sqlx::migrate! embeds the Search ledger migrations at compile time.
    // Cargo must rebuild this crate when a new migration file is added.
    println!("cargo:rerun-if-changed=migrations");
}
