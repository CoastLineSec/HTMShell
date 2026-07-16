use crate::{ExperimentalNodeIdentity, RuntimeError};
use blitz_html::HtmlDocument;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy)]
struct SlotState {
    generation: u64,
    live: bool,
}

/// Diagnostic-only identity registry layered over Blitz's reusable slab slots.
///
/// This is deliberately private and is not a proposed HTMShell NodeId API.
#[derive(Debug, Clone)]
pub(crate) struct IdentityRegistry {
    slots: BTreeMap<usize, SlotState>,
}

impl IdentityRegistry {
    pub(crate) fn from_document(document: &HtmlDocument) -> Self {
        Self {
            slots: author_slots(document)
                .into_iter()
                .map(|slot| {
                    (
                        slot,
                        SlotState {
                            generation: 0,
                            live: true,
                        },
                    )
                })
                .collect(),
        }
    }

    pub(crate) fn identity_for_slot(
        &self,
        document: &HtmlDocument,
        slot: usize,
    ) -> Result<ExperimentalNodeIdentity, RuntimeError> {
        let Some(state) = self.slots.get(&slot) else {
            return Err(RuntimeError::InvalidMutationTarget(format!(
                "Blitz slot {slot} has not been registered"
            )));
        };
        let identity = ExperimentalNodeIdentity {
            slot,
            generation: state.generation,
        };
        if !state.live || document.get_node(slot).is_none() {
            return Err(RuntimeError::StaleIdentity {
                slot,
                generation: state.generation,
            });
        }
        Ok(identity)
    }

    pub(crate) fn activate_created(
        &mut self,
        document: &HtmlDocument,
        slot: usize,
    ) -> Result<ExperimentalNodeIdentity, RuntimeError> {
        if document.get_node(slot).is_none() {
            return Err(RuntimeError::InvalidMutationTarget(format!(
                "new Blitz slot {slot} is not live"
            )));
        }
        let was_known = self.slots.contains_key(&slot);
        let state = self.slots.entry(slot).or_insert(SlotState {
            generation: 0,
            live: false,
        });
        if state.live {
            return Err(RuntimeError::InvalidMutationTarget(format!(
                "Blitz slot {slot} is already live"
            )));
        }
        if state.generation == u64::MAX {
            return Err(RuntimeError::LimitExceeded(format!(
                "identity generation exhausted for Blitz slot {slot}"
            )));
        }
        if was_known {
            state.generation = state.generation.saturating_add(1);
        }
        state.live = true;
        Ok(ExperimentalNodeIdentity {
            slot,
            generation: state.generation,
        })
    }

    pub(crate) fn subtree_slots(
        &self,
        document: &HtmlDocument,
        root: ExperimentalNodeIdentity,
    ) -> Result<Vec<usize>, RuntimeError> {
        let root_slot = self.resolve(document, root)?;
        let mut slots = Vec::new();
        let mut stack = vec![root_slot];
        while let Some(slot) = stack.pop() {
            let node = document.get_node(slot).ok_or(RuntimeError::StaleIdentity {
                slot: root.slot,
                generation: root.generation,
            })?;
            slots.push(slot);
            stack.extend(node.children.iter().rev().copied());
        }
        Ok(slots)
    }

    pub(crate) fn retire_removed(
        &mut self,
        document: &HtmlDocument,
        removed_slots: &[usize],
    ) -> Result<Vec<ExperimentalNodeIdentity>, RuntimeError> {
        let mut removed = Vec::with_capacity(removed_slots.len());
        for slot in removed_slots {
            if document.get_node(*slot).is_some() {
                return Err(RuntimeError::InvalidMutationTarget(format!(
                    "removed Blitz slot {slot} is still live"
                )));
            }
            let state = self.slots.get_mut(slot).ok_or_else(|| {
                RuntimeError::InvalidMutationTarget(format!(
                    "removed Blitz slot {slot} was never registered"
                ))
            })?;
            if !state.live {
                return Err(RuntimeError::StaleIdentity {
                    slot: *slot,
                    generation: state.generation,
                });
            }
            removed.push(ExperimentalNodeIdentity {
                slot: *slot,
                generation: state.generation,
            });
            state.live = false;
        }
        Ok(removed)
    }

    pub(crate) fn resolve(
        &self,
        document: &HtmlDocument,
        identity: ExperimentalNodeIdentity,
    ) -> Result<usize, RuntimeError> {
        let Some(state) = self.slots.get(&identity.slot) else {
            return Err(RuntimeError::StaleIdentity {
                slot: identity.slot,
                generation: identity.generation,
            });
        };
        if !state.live
            || state.generation != identity.generation
            || document.get_node(identity.slot).is_none()
        {
            return Err(RuntimeError::StaleIdentity {
                slot: identity.slot,
                generation: identity.generation,
            });
        }
        Ok(identity.slot)
    }

    pub(crate) fn live_identities(
        &self,
        document: &HtmlDocument,
    ) -> Result<BTreeMap<usize, ExperimentalNodeIdentity>, RuntimeError> {
        author_slots(document)
            .into_iter()
            .map(|slot| self.identity_for_slot(document, slot).map(|id| (slot, id)))
            .collect()
    }
}

pub(crate) fn author_slots(document: &HtmlDocument) -> Vec<usize> {
    let mut slots = Vec::new();
    let mut seen = BTreeSet::new();
    let mut stack = vec![0usize];
    while let Some(slot) = stack.pop() {
        if !seen.insert(slot) {
            continue;
        }
        let Some(node) = document.get_node(slot) else {
            continue;
        };
        slots.push(slot);
        stack.extend(node.children.iter().rev().copied());
    }
    slots
}

#[cfg(test)]
mod tests {
    use super::*;
    use blitz_dom::{DocumentConfig, LocalName, QualName, ns};
    use blitz_html::HtmlProvider;
    use std::sync::Arc;

    fn element_name(local: &str) -> QualName {
        QualName {
            prefix: None,
            ns: ns!(html),
            local: LocalName::from(local),
        }
    }

    fn document() -> HtmlDocument {
        let mut document = HtmlDocument::from_html(
            "<!doctype html><html><head></head><body><main id=\"root\"></main></body></html>",
            DocumentConfig {
                html_parser_provider: Some(Arc::new(HtmlProvider)),
                ..Default::default()
            },
        );
        document.set_incremental_layout(true);
        document.resolve(0.0);
        document
    }

    #[test]
    fn removed_identity_is_stale_after_slot_reuse() {
        let mut document = document();
        let parent = document.query_selector("#root").unwrap().unwrap();
        let mut registry = IdentityRegistry::from_document(&document);
        let first_slot = document
            .mutate()
            .create_element(element_name("div"), Vec::new());
        document.mutate().append_children(parent, &[first_slot]);
        let first = registry.activate_created(&document, first_slot).unwrap();
        let subtree = registry.subtree_slots(&document, first).unwrap();
        assert!(document.mutate().remove_and_drop_node(first_slot).is_some());
        registry.retire_removed(&document, &subtree).unwrap();
        assert!(matches!(
            registry.resolve(&document, first),
            Err(RuntimeError::StaleIdentity { .. })
        ));

        let second_slot = document
            .mutate()
            .create_element(element_name("div"), Vec::new());
        document.mutate().append_children(parent, &[second_slot]);
        let second = registry.activate_created(&document, second_slot).unwrap();
        assert_eq!(
            first.slot, second.slot,
            "Blitz should reuse the vacant slab slot"
        );
        assert!(second.generation > first.generation);
        assert!(matches!(
            registry.resolve(&document, first),
            Err(RuntimeError::StaleIdentity { .. })
        ));
        assert_eq!(registry.resolve(&document, second).unwrap(), second_slot);
    }

    #[test]
    fn repeated_reuse_never_aliases_an_old_identity() {
        let mut document = document();
        let parent = document.query_selector("#root").unwrap().unwrap();
        let mut registry = IdentityRegistry::from_document(&document);
        let mut stale = Vec::new();
        for _ in 0..32 {
            let slot = document
                .mutate()
                .create_element(element_name("div"), Vec::new());
            document.mutate().append_children(parent, &[slot]);
            let identity = registry.activate_created(&document, slot).unwrap();
            assert!(
                stale
                    .iter()
                    .all(|old| registry.resolve(&document, *old).is_err())
            );
            let subtree = registry.subtree_slots(&document, identity).unwrap();
            assert!(document.mutate().remove_and_drop_node(slot).is_some());
            registry.retire_removed(&document, &subtree).unwrap();
            stale.push(identity);
        }
        assert!(stale.windows(2).all(|pair| pair[0] != pair[1]));
    }
}
