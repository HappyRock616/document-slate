//! The paper sizes every app's File › Paper dropdown offers (same list, same order everywhere).

/// (shown name, width × height in points, portrait).
pub const PAPERS: [(&str, f32, f32); 4] = [
    ("Letter", 612.0, 792.0),
    ("Legal", 612.0, 1008.0),
    ("A4", 595.28, 841.89),
    ("11×17", 792.0, 1224.0),
];

/// The listed paper that a page of `w` × `h` points is (either way round), if any.
pub fn name_for(w: f32, h: f32) -> Option<&'static str> {
    let (a, b) = (w.min(h), w.max(h));
    PAPERS.iter().find(|p| (p.1 - a).abs() < 3.0 && (p.2 - b).abs() < 3.0).map(|p| p.0)
}

/// Menu label with a dot in front of the current choice.
pub fn label(name: &str, current: bool) -> String {
    if current { format!("• {name}") } else { format!("   {name}") }
}
