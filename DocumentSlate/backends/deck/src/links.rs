//! Hyperlinks and click actions on a slide, for a tap in the viewer or the slideshow.
//!
//! Read-only: nothing here runs an action. The engine's renderer places every text hyperlink
//! (with its slide-space box) and every shape's own action (Insert › Action); [`DeckDoc::link_at`]
//! says which one a point is on, the same way upstream's slideshow decides a click: a text link
//! first, else the topmost shape under the point — its own action, or its group's — and a shape
//! without an action hides the ones beneath it.

use deckcraft_geom::Rect;
use deckcraft_model::text::{Action, Hyperlink};

use crate::DeckDoc;

/// Where a link leads.
#[derive(Clone, Debug, PartialEq)]
pub enum DeckLink {
    /// A web, mail or other address, as written in the file.
    Url(String),
    /// Another slide of this deck (0-based index).
    Slide(usize),
    NextSlide,
    PreviousSlide,
    FirstSlide,
    LastSlide,
    EndShow,
    /// An action Deck Slate doesn't run (program, custom show, media…): what it is, in words.
    Other(String),
}

impl DeckDoc {
    /// The link or click action at slide point (x, y) (points, from the top-left) on slide
    /// `slide` (0-based), if any.
    pub fn link_at(&self, slide: usize, x: f64, y: f64) -> Option<DeckLink> {
        let st = self.state().ok()?;
        let pres = &st.doc;
        let placed = deckcraft_render::place_slide(pres, slide);
        // A little slack (1 pt) so a finger on the edge still counts.
        let inside = |r: &Rect| x >= r.x0 - 1.0 && x <= r.x1 + 1.0 && y >= r.y0 - 1.0 && y <= r.y1 + 1.0;
        // Text links: the last placed is drawn on top.
        if let Some(l) = placed.links.iter().rev().find(|l| l.text && inside(&l.rect)) {
            return Some(convert(pres, &l.link));
        }
        // The topmost (non-group) shape under the point, then the groups it is in.
        let top = placed.shapes.iter().rposition(|s| !s.is_group && inside(&s.rect))?;
        let mut at = Some(top);
        while let Some(i) = at {
            let s = &placed.shapes[i];
            if let Some(link) = &s.link {
                return Some(convert(pres, link));
            }
            at = s.parent;
        }
        None
    }
}

fn convert(pres: &deckcraft_model::Presentation, link: &Hyperlink) -> DeckLink {
    match &link.action {
        Action::Url { url } => DeckLink::Url(url.clone()),
        Action::Slide { slide } => match pres.slide_index(*slide) {
            Some(i) => DeckLink::Slide(i),
            None => DeckLink::Other("a slide that isn't in this deck".into()),
        },
        Action::NextSlide => DeckLink::NextSlide,
        Action::PreviousSlide => DeckLink::PreviousSlide,
        Action::FirstSlide => DeckLink::FirstSlide,
        Action::LastSlide => DeckLink::LastSlide,
        Action::EndShow => DeckLink::EndShow,
        Action::LastViewed => DeckLink::Other("the last slide viewed".into()),
        Action::CustomShow { name, .. } => DeckLink::Other(format!("the custom show \"{name}\"")),
        Action::Program { path } => DeckLink::Other(format!("a program ({path})")),
        Action::PlayMedia => DeckLink::Other("media".into()),
    }
}
