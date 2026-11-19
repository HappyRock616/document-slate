//! Hyperlinks in WordSlate's viewer: a tap on linked text (or a linked picture, shape or text box)
//! shows the shared link pop-up (`crate::links::show_link`); a link to a place in this document
//! (`#bookmark`, a `REF … \h` cross-reference) scrolls there instead.
//!
//! Same rules as upstream's desktop Ctrl+click (`ui-egui/src/canvas/links.rs`): the link only
//! counts when the point is on the linked character itself, not beside or below it.

use super::*;
use wordcraft_engine::doc::para::InlineObject;
use wordcraft_engine::doc::Pos;

impl WordViewer {
    /// The hyperlink under page point (x, y) on `page`, as written in the file
    /// (`https://…`, `mailto:…`, `#bookmark`).
    pub fn link_at(&self, page: usize, x: f32, y: f32) -> Option<String> {
        // A linked picture or shape lies over the text: it comes first.
        if let Some(hit) = self.layout.object_at(page, x, y, 2.0) {
            let pos = Pos { story: hit.story, path: hit.path.clone(), off: hit.off };
            if let Some(link) = self.char_link(&pos) {
                return Some(link);
            }
        }
        let story = self.layout.story_at(page, x, y).unwrap_or(StoryRef::Body);
        // A linked text box: its own link before its text's.
        if let StoryRef::Part(id) = story {
            if let Some(link) = self.layout.text_box(id, page).and_then(|o| self.char_link(&Pos { story: o.story, path: o.path.clone(), off: o.off })) {
                return Some(link);
            }
        }
        let pos = self.layout.hit(page, x, y, story)?;
        self.text_link_under(page, x, y, &pos)
    }

    /// The link on the character at `pos` (an object's placeholder character).
    fn char_link(&self, pos: &Pos) -> Option<String> {
        self.session.doc.para_at(pos)?.props_of_char(pos.off).link.clone().filter(|l| !l.is_empty())
    }

    /// The link of the character on either side of caret position `pos`, if (x, y) is on it.
    fn text_link_under(&self, page: usize, x: f32, y: f32, pos: &Pos) -> Option<String> {
        let doc = &self.session.doc;
        let para = doc.para_at(pos)?;
        [(pos.off, para.next_boundary(pos.off)), (para.prev_boundary(pos.off), pos.off)].into_iter().filter(|(a, b)| a < b).find_map(|(a, b)| {
            let link = para.props_of_char(a).link.clone().filter(|l| !l.is_empty()).or_else(|| field_link(para.object_at(a)?))?;
            let (from, to) = (Pos { off: a, ..pos.clone() }, Pos { off: b, ..pos.clone() });
            // A little slack (1 pt) so a finger on the edge of a letter still counts.
            self.layout
                .selection_rects(doc, &from, &to, page)
                .into_iter()
                .any(|(pi, r)| pi == page && x >= r.x - 1.0 && x <= r.x + r.w + 1.0 && y >= r.y - 1.0 && y <= r.y + r.h + 1.0)
                .then_some(link)
        })
    }

    /// Viewer tap at a page point: follow the link there, if any. Returns true when there was one.
    pub(super) fn follow_link_at(&mut self, page: usize, x: f32, y: f32) -> bool {
        let Some(link) = self.link_at(page, x, y) else { return false };
        self.follow_link(&link);
        true
    }

    /// A place in this document scrolls there; anything else shows the link pop-up.
    pub fn follow_link(&mut self, link: &str) {
        if let Some(name) = link.strip_prefix('#') {
            if self.go_to_bookmark(name) {
                return;
            }
        }
        crate::links::show_link(link);
    }

    /// Scroll to bookmark `name`. The viewer doesn't use the selection, so it's put back.
    fn go_to_bookmark(&mut self, name: &str) -> bool {
        let saved = self.session.sel.clone();
        let ok = self.session.run("edit.goto", &serde_json::json!({ "bookmark": name })).is_ok();
        let target = ok.then(|| self.session.sel.ordered().0.clone());
        if !self.editing {
            self.session.sel = saved;
        }
        let Some(pos) = target else { return false };
        match self.layout.caret(&pos) {
            Some(c) => {
                self.jump_to = Some((c.page, c.top));
                if self.editing {
                    self.session.sel = Selection::caret(pos);
                    self.after_change();
                }
                true
            }
            None => false,
        }
    }
}

/// The link a cross-reference field makes: `REF name \h` and `PAGEREF name \h` lead to the
/// bookmark `name` (`#name`), as in Word.
fn field_link(obj: &InlineObject) -> Option<String> {
    let InlineObject::Field { instr, .. } = obj else { return None };
    let code = wordcraft_engine::cmd::references::FieldCode::parse(instr);
    let name = code.args.first().filter(|n| !n.is_empty())?;
    (matches!(code.name.as_str(), "REF" | "PAGEREF") && code.has('h')).then(|| format!("#{name}"))
}
