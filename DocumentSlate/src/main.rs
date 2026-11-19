// Desktop launcher, for quick testing on Windows without an emulator.
// Usage: document-slate-desktop [path\to\file.docx]   (no path = built-in sample)
fn main() -> eframe::Result {
    env_logger::init();
    document_slate::run_desktop(std::env::args_os().nth(1).map(Into::into))
}
