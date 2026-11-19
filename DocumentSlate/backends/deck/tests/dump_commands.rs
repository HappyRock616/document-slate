//! Developer helper (not a real test): writes every engine command id with its parameter
//! description to _cleanup/deck-commands.txt, so we can see what the engine can do.
//! Run with: cargo test --test dump_commands -- --ignored
#[test]
#[ignore]
fn dump_commands() {
    let mut out = String::new();
    for c in deckcraft_engine::command_specs() {
        out.push_str(&format!("{}  [{}]  {}\n", c.id, c.label, c.params));
    }
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../_cleanup");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("deck-commands.txt"), out).unwrap();
}
