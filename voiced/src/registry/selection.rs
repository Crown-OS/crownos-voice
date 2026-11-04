use std::cmp::Reverse;
use std::fmt::Display;

/// What ranking needs to know about one backend for one workload.
#[derive(Clone, Copy, Debug)]
pub struct Candidate {
    pub score: u32,
    /// Position in the installer's preferred order; `None` when unlisted.
    pub preference: Option<usize>,
}

/// Usable candidates, best first: highest score, ties broken by preference.
pub fn rank(candidates: &[Candidate]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..candidates.len())
        .filter(|&index| candidates[index].score > 0)
        .collect();
    order.sort_by_key(|&index| {
        let candidate = candidates[index];
        (
            Reverse(candidate.score),
            candidate.preference.unwrap_or(usize::MAX),
        )
    });
    order
}

/// Tries `order` in turn; returns the offset into `order` that succeeded and its result.
pub fn first_created<T, E: Display>(
    order: &[usize],
    mut create: impl FnMut(usize) -> Result<T, E>,
) -> Option<(usize, T)> {
    order
        .iter()
        .enumerate()
        .find_map(|(offset, &index)| match create(index) {
            Ok(created) => Some((offset, created)),
            Err(error) => {
                log::warn!("backend {index} failed, trying the next one: {error}");
                None
            }
        })
}

/// A stream's ranked backends and which one is serving it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BackendChoice {
    order: Vec<usize>,
    position: usize,
}

impl BackendChoice {
    pub fn new(order: Vec<usize>, position: usize) -> Self {
        Self { order, position }
    }

    pub fn current(&self) -> usize {
        self.order[self.position]
    }

    /// Backends ranked below the current one.
    pub fn fallbacks(&self) -> &[usize] {
        &self.order[self.position + 1..]
    }

    /// The same ranking, moved to `offset` within [`Self::fallbacks`].
    pub fn demoted(&self, offset: usize) -> Self {
        Self {
            order: self.order.clone(),
            position: self.position + 1 + offset,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(score: u32, preference: Option<usize>) -> Candidate {
        Candidate { score, preference }
    }

    #[test]
    fn ranks_by_score_and_drops_unusable_backends() {
        let candidates = [
            candidate(40, None),
            candidate(0, Some(0)),
            candidate(120, None),
            candidate(1, None),
        ];
        assert_eq!(rank(&candidates), [2, 0, 3]);
    }

    #[test]
    fn preference_only_breaks_ties() {
        let candidates = [
            candidate(50, None),
            candidate(50, Some(1)),
            candidate(50, Some(0)),
            candidate(60, Some(5)),
        ];
        assert_eq!(rank(&candidates), [3, 2, 1, 0]);
    }

    #[test]
    fn falls_back_down_the_order_on_create_failure() {
        let mut attempted = Vec::new();
        let created = first_created(&[4, 2, 7], |index| {
            attempted.push(index);
            if index == 7 {
                Ok("seven")
            } else {
                Err("unavailable")
            }
        });
        assert_eq!(created, Some((2, "seven")));
        assert_eq!(attempted, [4, 2, 7]);
    }

    #[test]
    fn reports_none_when_every_backend_fails() {
        assert_eq!(
            first_created(&[1, 2], |_| Err::<(), _>("unavailable")),
            None
        );
    }

    #[test]
    fn demotion_walks_the_remaining_backends() {
        let choice = BackendChoice::new(vec![5, 3, 8, 1], 0);
        assert_eq!(choice.current(), 5);
        assert_eq!(choice.fallbacks(), [3, 8, 1]);
        let choice = choice.demoted(1);
        assert_eq!(choice.current(), 8);
        assert_eq!(choice.fallbacks(), [1]);
        assert!(choice.demoted(0).fallbacks().is_empty());
    }
}
