//! Refresh icon that keeps turning through at least half a revolution.
//! `Spinner`'s cycle is 0.8s and not configurable; a linear ease makes 400ms a half turn.

use std::time::{Duration, Instant};

use gpui_kit::component::button::Button;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{Sizable as _, Size};
use gpui_kit::{Context, ElementId};

use crate::assets::lucide;

/// Half a turn of the refresh icon.
pub const HALF_TURN: Duration = Duration::from_millis(400);

/// Remembers the instant a half turn ends, so a fast load still finishes the turn.
#[derive(Clone, Copy, Default)]
pub struct HalfSpin {
    until: Option<Instant>,
}

impl HalfSpin {
    /// Start (or restart) the minimum half turn.
    pub fn begin(&mut self) {
        self.until = Some(Instant::now() + HALF_TURN);
    }

    /// `busy` keeps the icon turning for the whole operation; `until` covers the tail.
    pub fn spinning(&self, busy: bool) -> bool {
        spinning_until(busy, self.until, Instant::now())
    }

    /// After the operation ends, repaint when the remaining half turn finishes.
    /// `slot` finds this spin on `T` — a newer `begin` must not be cleared by the old timer.
    pub fn settle<T: 'static>(&mut self, cx: &mut Context<T>, slot: fn(&mut T) -> &mut Self) {
        let Some(until) = self.until else {
            return;
        };
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            self.until = None;
            return;
        }
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(left).await;
            let _ = this.update(cx, |this, cx| {
                let spin = slot(this);
                if spin.until.is_some_and(|t| Instant::now() < t) {
                    return;
                }
                spin.until = None;
                cx.notify();
            });
        })
        .detach();
    }
}

pub fn spinning_until(busy: bool, until: Option<Instant>, now: Instant) -> bool {
    busy || until.is_some_and(|t| now < t)
}

/// Refresh button. While `spinning`, the icon is a linear `Spinner` instead of a static glyph.
/// Callers add style, tooltip, and the click handler. Do not disable it while it spins:
/// the disabled style fades the icon, so the turn looks like it never started.
pub fn refresh_button(id: impl Into<ElementId>, spinning: bool, size: Size) -> Button {
    let glyph = lucide("refresh-cw");
    let mut button = Button::new(id).with_size(size);
    // Icon slot, so a label stays to the right of the turning glyph.
    button = if spinning {
        button.icon(Spinner::new().icon(glyph).with_size(size).ease(|t| t))
    } else {
        button.icon(glyph)
    };
    button
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_keeps_spinning_for_half_a_turn_after_the_load() {
        let start = Instant::now();
        let until = start + HALF_TURN;
        assert!(spinning_until(true, Some(until), start));
        assert!(spinning_until(
            false,
            Some(until),
            start + Duration::from_millis(100)
        ));
        assert!(!spinning_until(false, Some(until), start + HALF_TURN));
        assert!(!spinning_until(false, None, start));
    }
}
