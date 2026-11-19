//! On-screen keyboard bridge (Android, GameActivity).
//!
//! winit 0.30 ignores GameActivity's text events, so the keyboard's typing never
//! reaches egui on its own. GameActivity does keep a "shadow" text buffer that
//! the keyboard edits (with suggestions, autocorrect, swipe…). Each frame we read
//! that buffer, diff it against last frame, and turn the difference into egui
//! events: deleted characters -> Backspace, new text -> Text, newline -> Enter.

use eframe::egui;
use winit::platform::android::activity::AndroidApp;
use winit::platform::android::activity::input::{
    ImeOptions, InputType, TextInputAction, TextInputState,
};

/// Reset the shadow buffer once it gets this long (and nothing is mid-composition).
const MAX_SHADOW_CHARS: usize = 200;

/// After a reset, the keyboard may still echo its old text for a few frames (and then the
/// clear arrives as a burst of deletions). Changes inside this window only re-sync.
const SETTLE: std::time::Duration = std::time::Duration::from_millis(250);

pub struct ImeBridge {
    app: AndroidApp,
    last: String,
    /// egui widget that had keyboard focus last frame (`None` = the document editors).
    focus: Option<egui::Id>,
    settle_until: Option<std::time::Instant>,
}

impl ImeBridge {
    pub fn new(app: AndroidApp) -> Self {
        // Ask for a normal multi-line text keyboard with autocorrect and sentence caps.
        app.set_ime_editor_info(
            InputType::TYPE_CLASS_TEXT
                | InputType::TYPE_TEXT_FLAG_MULTI_LINE
                | InputType::TYPE_TEXT_FLAG_AUTO_CORRECT
                | InputType::TYPE_TEXT_FLAG_CAP_SENTENCES,
            TextInputAction::None,
            ImeOptions::IME_FLAG_NO_FULLSCREEN,
        );
        let bridge = Self { app, last: String::new(), focus: None, settle_until: None };
        bridge.app.set_text_input_state(TextInputState::default());
        bridge
    }

    /// Empty the shadow buffer (e.g. after the user taps somewhere else in the text).
    fn reset(&mut self) {
        self.last.clear();
        self.app.set_text_input_state(TextInputState::default());
    }

    /// Reset, then ignore the keyboard's late echoes for a moment.
    fn reset_and_settle(&mut self) {
        self.reset();
        self.settle_until = Some(std::time::Instant::now() + SETTLE);
    }

    /// Called from eframe's `raw_input_hook`, before egui handles this frame's input.
    pub fn pump(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        // Focus moved to another text box (or a card with one closed): what the keyboard holds
        // belongs to the old box. Without this, closing Insert › Barcode sent the typed
        // address back as Backspaces into the document.
        let focus = ctx.memory(|m| m.focused());
        if focus != self.focus {
            self.focus = focus;
            self.reset_and_settle();
            return;
        }
        // A touch inside the app may move egui's cursor; the keyboard's buffer would
        // then be out of sync, so start fresh.
        let touched = raw.events.iter().any(|e| {
            matches!(e, egui::Event::PointerButton { pressed: true, .. })
                || matches!(e, egui::Event::Touch { phase: egui::TouchPhase::Start, .. })
        });
        if touched {
            self.reset_and_settle();
            return;
        }

        let state = self.app.text_input_state();
        if state.text == self.last {
            return;
        }
        if self.settle_until.is_some_and(|t| std::time::Instant::now() < t) {
            // A late echo of the old text: re-sync instead of turning it into typing.
            if state.compose_region.is_none() && !state.text.is_empty() {
                self.reset();
            } else {
                self.last = state.text;
            }
            return;
        }
        self.settle_until = None;

        let old: Vec<char> = self.last.chars().collect();
        let new: Vec<char> = state.text.chars().collect();
        let common = old.iter().zip(&new).take_while(|(a, b)| a == b).count();

        for _ in common..old.len() {
            push_key(raw, egui::Key::Backspace);
        }
        let added: String = new[common..].iter().collect();
        for (i, part) in added.split('\n').enumerate() {
            if i > 0 {
                push_key(raw, egui::Key::Enter);
            }
            if !part.is_empty() {
                raw.events.push(egui::Event::Text(part.to_owned()));
            }
        }

        self.last = state.text;
        if state.compose_region.is_none() && new.len() > MAX_SHADOW_CHARS {
            self.reset();
        }
    }
}

fn push_key(raw: &mut egui::RawInput, key: egui::Key) {
    for pressed in [true, false] {
        raw.events.push(egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
    }
}
