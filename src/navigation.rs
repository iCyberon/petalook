#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Boundary {
    Beginning,
    End,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Navigation {
    pub index: usize,
    pub direction: i32,
    pub boundary: Option<Boundary>,
}

impl Navigation {
    pub fn step(index: usize, count: usize, delta: i32) -> Option<Self> {
        if count < 2 || delta == 0 {
            return None;
        }
        let target = index as i128 + delta as i128;
        Some(Self {
            index: target.rem_euclid(count as i128) as usize,
            direction: delta.signum(),
            boundary: if target < 0 {
                Some(Boundary::Beginning)
            } else if target >= count as i128 {
                Some(Boundary::End)
            } else {
                None
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_in_both_directions_without_reversing_animation() {
        assert_eq!(
            Navigation::step(3, 4, 1),
            Some(Navigation {
                index: 0,
                direction: 1,
                boundary: Some(Boundary::End),
            })
        );
        assert_eq!(
            Navigation::step(0, 4, -1),
            Some(Navigation {
                index: 3,
                direction: -1,
                boundary: Some(Boundary::Beginning),
            })
        );
        assert_eq!(Navigation::step(1, 4, 1).unwrap().boundary, None);
        assert_eq!(Navigation::step(0, 4, -9).unwrap().index, 3);
        assert_eq!(Navigation::step(3, 4, 9).unwrap().index, 0);
    }

    #[test]
    fn empty_and_single_file_lists_do_not_transition() {
        assert!(Navigation::step(0, 0, 1).is_none());
        assert!(Navigation::step(0, 1, -1).is_none());
        assert!(Navigation::step(0, 2, 0).is_none());
    }
}
