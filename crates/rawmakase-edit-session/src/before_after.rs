//! Lightroom's Before/After settings: what Before shows beside the edit, and
//! the commands that copy or swap settings between them. Before belongs to the
//! open photo and is not saved with the edit, as in Lightroom; how the two are
//! laid out on screen is the window's.
use crate::history::Step;
use rawmakase_model::model::recipe::Recipe;

/// What a Before/After command does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transfer {
    /// Before's settings become the edit's, as one History step.
    BeforeToAfter,
    /// The edit's settings become Before's.
    AfterToBefore,
    /// Each takes the other's settings; the edit's change is one History step.
    Swap,
}

/// What a [`Transfer`] changes.
#[derive(Clone, Debug, PartialEq)]
pub struct Transferred {
    /// Before's new settings, when it takes the edit's.
    pub before: Option<Recipe>,
    /// The edit's new settings and the History step that records them, when
    /// they change.
    pub edit: Option<(Step, Recipe)>,
}

/// Before takes the edit's crop and orientation, so both sides show the same
/// part of the photo, as the full-frame Before always has.
pub fn framed_like(mut before: Recipe, edit: &Recipe) -> Recipe {
    before.crop = edit.crop;
    before.rotation = edit.rotation;
    before.flip_x = edit.flip_x;
    before.flip_y = edit.flip_y;
    before.straighten = edit.straighten;
    // Whether that angle is Auto's goes with it.
    before.straighten_auto = edit.straighten_auto;
    before
}

/// `transfer` between `before`, Before's settings framed like the edit, and
/// `after`, the edit's.
pub fn transfer(transfer: Transfer, before: Recipe, after: &Recipe) -> Transferred {
    let (before, edit) = match transfer {
        Transfer::AfterToBefore => (Some(after.clone()), None),
        Transfer::BeforeToAfter => (None, Some(("Copy Before Settings to After", before))),
        Transfer::Swap => (
            Some(after.clone()),
            Some(("Swap Before and After Settings", before)),
        ),
    };
    Transferred {
        before,
        edit: edit
            .filter(|(_, e)| e != after)
            .map(|(step, e)| (Step::new(step, ""), e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swapping_gives_each_side_the_others_settings_and_copying_the_same_records_nothing() {
        let before = Recipe {
            exposure: -1.,
            ..Recipe::default()
        };
        let after = Recipe {
            exposure: 1.,
            crop: [0.1, 0.1, 0.9, 0.9],
            ..Recipe::default()
        };
        let framed = framed_like(before, &after);
        assert_eq!(framed.crop, after.crop);
        let swapped = transfer(Transfer::Swap, framed.clone(), &after);
        assert_eq!(swapped.before.as_ref(), Some(&after));
        let (step, edit) = swapped.edit.unwrap();
        assert_eq!(
            (step.name.as_str(), edit.exposure),
            ("Swap Before and After Settings", -1.)
        );
        let copied = transfer(Transfer::AfterToBefore, framed, &after);
        assert_eq!((copied.before, copied.edit), (Some(after.clone()), None));
        let same = transfer(Transfer::BeforeToAfter, after.clone(), &after);
        assert_eq!(
            same,
            Transferred {
                before: None,
                edit: None
            }
        );
    }
}
