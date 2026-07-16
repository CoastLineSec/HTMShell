# ADR 0002: Test incremental mutation before platform integration

- Status: experimental and reversible
- Date: 2026-07-16
- Decision: CONTINUE WITH NARROWER PROFILE

## Context

Gate A showed that modular Blitz crates can provide a headless HTML, CSS, layout, text, and paint pipeline without a browser shell or prohibited platform dependency. A desktop shell also needs a long-running document: rebuilding the HTML document after every state change would invalidate stable identity, make fine-grained diagnostics difficult, and weaken future input and compositor transport work.

Gate A.1 therefore tests mutation before Wayland. This isolates document-lifetime, style-reload, identity, diffing, and paint-cost questions from surface presentation and compositor protocol questions.

## Experimental decision

Keep Blitz pinned at commit `389e3762fc0ac19f6de7c0cec7201d0c8bde393a`. Mutate the existing Blitz document through its host APIs for text, attributes, insertion, and removal. Replace the author stylesheet attached to the existing `<link>` owner only after a detached Stylo parse pass reports no errors. Missing or malformed candidates leave the accepted sheet and snapshot untouched.

HTMShell owns an experimental `{slot, generation}` identity registry because Blitz slab slots are reusable implementation details. Removal retires the current generation; reactivation of a reused slot increments it. The resulting deterministic snapshot and diff are diagnostic infrastructure, not the final scene IR, protocol identity, or public runtime API.

Every accepted paint phase continues to invoke `blitz_paint::paint_scene`. The diagnostic diff does not hide or replace that full AnyRender scene reconstruction, and the spike records paint cost separately.

## Acceptance criteria

- Parse the document once and retain its engine identity through all phases.
- Mutate text and classes and insert/remove nodes without document reconstruction.
- Retain unaffected identities, reject stale references, and prevent reused slots from aliasing old identities.
- Replace a valid author stylesheet without replacing the DOM.
- Preserve the last-known-good stylesheet and accepted scene after missing or malformed candidates.
- Produce deterministic snapshots and diffs that distinguish created, removed, retained, and changed nodes.
- Expose available dirty/damage/snapshot evidence and mark unavailable engine counters as unavailable.
- Measure full paint reconstruction on approximately 120-, 1,000-, and 5,000-node fixtures.
- Preserve the Gate A dependency boundary and require no Blitz fork or patch.

## Stop conditions

Stop rather than expand scope if ordinary mutation reparses HTML, stylesheet replacement reconstructs the DOM, safe removal requires broad upstream changes, generational identity cannot reject stale slots, failed reload corrupts accepted state, or mutation introduces Dioxus, `blitz-shell`, `winit`, networking, a platform toolkit, or a replacement CSS engine.

## Consequences and limitations

The host-driven document model is viable for another controlled gate. DOM identity retention, HTMShell-owned scene differencing, and full Blitz painting remain separate facts. Blitz does not expose exact counts for restyled nodes, recomputed Taffy nodes, regenerated paint commands, or retained paint nodes through the selected APIs.

Candidate CSS validation is deliberately conservative: any reported Stylo parse error rejects the whole replacement. The candidate is prepared before the accepted sheet changes, but Blitz does not expose a transactional commit/rollback primitive if the in-process stylesheet swap itself were to panic. The current trusted, local experiment catches runtime panics at the adapter boundary; it does not claim atomic recovery from arbitrary engine failure.

The damage union is headless diagnostic evidence only. It omits paint expansion from shadows, filters, antialiasing, transforms, backdrop or material sampling, renderer behavior, and compositor damage rules.

## Result

The document remained intact through text and class mutation, dynamic insertion/removal, repeated slab-slot reuse, valid author-sheet replacement, same-document reapplication of that sheet, and rejected missing/malformed replacements. Deterministic snapshots, diffs, and local PNG output were reproduced across repeated runs. Approximately 120-, 1,000-, and 5,000-node fixtures completed without a prohibited dependency or upstream patch.

The decision is **CONTINUE WITH NARROWER PROFILE**. The long-running host model is credible, but CSS reload remains conservative and ordinary foreground paint is still fully reconstructed. This authorizes an independent compositor-transport/custom-surface feasibility gate; it does not authorize broad HTML/CSS feature expansion or declare Blitz permanent.
