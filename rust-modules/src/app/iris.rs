//! **IrisPlex's hidden door to Settings** (`plx_base::paths::iris_mode`).
//!
//! IrisPlex has no top bar, so the profile chip and its Settings row are out of reach on purpose.
//! A grown-up still needs Settings once in a while (sign out, switch profile, playback options), so
//! the remote combination UP UP DOWN DOWN LEFT RIGHT LEFT RIGHT, pressed on Home within
//! [`WINDOW_MS`], opens it. Eight presses in that order is not something a child mashing the
//! D-pad lands on, and every press but the last still does what it would have done anyway.

use plx_ui::consts::Key;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Dir {
    Up,
    Down,
    Left,
    Right,
}

const COMBO: [Dir; 8] = [Dir::Up, Dir::Up, Dir::Down, Dir::Down, Dir::Left, Dir::Right, Dir::Left, Dir::Right];

/// The whole combination must land inside this, measured from its first press.
pub(crate) const WINDOW_MS: u32 = 6_000;

/// The last [`COMBO`]-length presses, oldest first (`None` for a key that is not a direction).
#[derive(Default)]
pub(crate) struct Combo {
    recent: std::collections::VecDeque<(Option<Dir>, u32)>,
}

impl Combo {
    /// One fresh press at `now` (ms). `true` exactly when it completes the combination: the last
    /// eight presses are [`COMBO`] and the first of them was at most [`WINDOW_MS`] ago.
    pub(crate) fn press(&mut self, key: Key, now: u32) -> bool {
        let dir = match key {
            Key::Up => Some(Dir::Up),
            Key::Down => Some(Dir::Down),
            Key::Left { alt: false } => Some(Dir::Left),
            Key::Right { alt: false } => Some(Dir::Right),
            _ => None,
        };
        if self.recent.len() == COMBO.len() {
            self.recent.pop_front();
        }
        self.recent.push_back((dir, now));
        let done = self.recent.len() == COMBO.len()
            && self.recent.iter().zip(COMBO).all(|(&(d, _), want)| d == Some(want))
            && now.wrapping_sub(self.recent[0].1) <= WINDOW_MS;
        if done {
            self.recent.clear();
        }
        done
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(s: &str) -> Vec<Key> {
        s.split_whitespace()
            .map(|k| match k {
                "u" => Key::Up,
                "d" => Key::Down,
                "l" => Key::Left { alt: false },
                "r" => Key::Right { alt: false },
                _ => Key::Ok,
            })
            .collect()
    }

    fn run(c: &mut Combo, s: &str, start: u32, step: u32) -> Vec<bool> {
        keys(s).into_iter().enumerate().map(|(i, k)| c.press(k, start + i as u32 * step)).collect()
    }

    #[test]
    fn the_combination_opens_on_its_last_press_only() {
        let mut c = Combo::default();
        let fired = run(&mut c, "u u d d l r l r", 1_000, 300);
        assert_eq!(fired, [false, false, false, false, false, false, false, true]);
    }

    #[test]
    fn a_wrong_press_or_a_slow_one_starts_over_and_a_restart_still_counts() {
        let mut c = Combo::default();
        assert!(!run(&mut c, "u u d d l r l l", 0, 100).contains(&true), "wrong last press");
        // The extra leading UP: "u u u d d …" — the third UP restarts as press two of a new try.
        assert!(run(&mut c, "u u u d d l r l r", 10_000, 100).last() == Some(&true));
        assert!(!run(&mut c, "u u d d l r l r", 20_000, 1_000).contains(&true), "7 s is too slow");
        assert!(!run(&mut c, "u u d d ok l r l r", 40_000, 100).contains(&true), "OK breaks it");
    }

    #[test]
    fn mashing_one_direction_never_opens_it() {
        let mut c = Combo::default();
        for s in ["l l l l l l l l l l", "r r r r r r r r", "u u u u u u u u u", "d d d d d d d d"] {
            assert!(!run(&mut c, s, 0, 50).contains(&true), "{s}");
        }
    }
}
