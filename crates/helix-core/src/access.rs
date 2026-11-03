//! What the probe may open: the game phase and the DNA kit (design doc §5).

use std::sync::Arc;

/// Samples the DNA kit holds.
pub const KIT_SIZE: usize = 3;

#[derive(Debug, Clone)]
pub struct Access {
    phase: u8,
    /// DNA ids, newest first.
    kit: Vec<u8>,
    /// DNA ids granted each clearance code 02-12 (index 0 = code 02).
    groups: Arc<Vec<Vec<u8>>>,
}

impl Default for Access {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

impl Access {
    pub fn new(groups: Vec<Vec<u8>>) -> Self {
        Self { phase: 1, kit: Vec::new(), groups: Arc::new(groups) }
    }

    pub fn phase(&self) -> u8 {
        self.phase
    }

    pub fn set_phase(&mut self, phase: u8) {
        self.phase = phase.clamp(1, 4);
    }

    pub fn kit(&self) -> &[u8] {
        &self.kit
    }

    pub(crate) fn kit_mut(&mut self) -> &mut Vec<u8> {
        &mut self.kit
    }

    /// Lingo's `Cleared`: `01` always opens, everything opens after phase 1,
    /// otherwise a sample from the code's clearance group is needed.
    pub fn grants(&self, code: &str) -> bool {
        if code == "01" || self.phase > 1 {
            return true;
        }
        let group = code.trim().parse::<usize>().ok().and_then(|n| n.checked_sub(2)).and_then(|i| self.groups.get(i));
        group.is_some_and(|g| self.kit.iter().any(|dna| g.contains(dna)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clearance() {
        let mut a = Access::new(vec![vec![1, 2, 10], vec![1, 2]]);
        assert!(a.grants("01"));
        assert!(!a.grants("02"));
        a.kit_mut().push(10);
        assert!(a.grants("02"), "10 is in code 02's group");
        assert!(!a.grants("03"));
        assert!(!a.grants("12"), "no group for 12 in this made-up list");
        a.set_phase(2);
        assert!(a.grants("03"), "phase 2 grants everything");
    }
}
