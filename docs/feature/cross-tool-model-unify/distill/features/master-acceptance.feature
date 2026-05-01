# =============================================================================
# cross-tool-model-unify — Master Acceptance Feature File
#
# Wave: DISTILL (5 of 6)
# Author: Quinn (nw-acceptance-designer)
# Date: 2026-04-30
# Feature: Brownfield extension on shipped modeltap v1
#
# Tag glossary (mirrors v1 master-acceptance.feature; tag set is a superset):
#   @walking-skeleton    -- the single scenario that proves the v1 promise becomes true
#   @release-1           -- "Walking Skeleton" (US-U1..U7)
#   @release-2           -- "Polish" (US-U8..U10)
#   @us-uN               -- traceability to user-stories.md story ID (U-prefix is per-feature)
#   @real-io             -- uses real filesystem; required for at least one per adapter
#   @adapter-integration -- proves a single driven adapter against real I/O
#   @kpi-instrumentation -- asserts JSONL log output (~/.modeltap/launch.log)
#   @k3-latency          -- asserts K3 timing budget (NFR-1)
#   @nfr-perf            -- asserts an NFR perf budget (NFR-2/NFR-3)
#   @cross-artifact      -- asserts cross-artifact consistency (shared-artifacts-registry.md)
#   @property            -- universal invariant (DELIVER may implement as proptest)
#   @skip                -- not in the walking skeleton; enabled one at a time during DELIVER
#
# Scenario count: 43. Walking-skeleton scenarios: 1. Skipped scenarios: 42.
# Walking-skeleton litmus: a stakeholder watches Devon launch, see the
# Dedup-able number stop lying, press [u] on a "=" row, confirm, and watch a
# real disk-saving hardlink unify happen. That single scenario is
# "Devon reclaims disk by unifying a duplicated model from the main view".
#
# Per Luna's prioritization, US-U1..U7 are P1 (the walking-skeleton release)
# and US-U8..U10 are P2 (the polish release). Within P1, ONE scenario is
# tagged @walking-skeleton; the rest of P1 are @skip until DELIVER enables
# them one at a time.
#
# Constraints (per parent agent contract):
#   - Real plugins, real fixtures, real tokio runtime. No mocks at this level.
#   - No new env-var seams introduced in this artifact. Existing v1 seams only.
#   - "Devon" vocabulary throughout — no Rust types in Gherkin.
# =============================================================================

Feature: Cross-tool model unify — make the v1 promise true
  As Devon Park, a small-team developer with multiple local AI tools installed
  I want modeltap to honestly tell me when models are duplicated across tools
  And let me reclaim disk by unifying them with one keypress from the main view
  So that the v1 "Dedup-able" number stops lying and the [u] hotkey actually works

  Background:
    Given a clean modeltap log directory at "${TMPDIR}/cross-tool-model-unify-${SCENARIO_ID}"
    And the existing v1 acceptance harness conventions (headless mode, scripted input, JSONL events)

  # ===========================================================================
  # WALKING SKELETON
  # ---------------------------------------------------------------------------
  # The single end-to-end slice that proves a stakeholder demo: launch ->
  # background hashing produces a non-zero Dedup-able number -> Devon presses
  # [u] from the main view (not Detail) -> confirms in the dialog -> a real
  # hardlink is created across two tools and the summary bar reflects the
  # reclaim, all without restarting modeltap.
  #
  # Touches: US-U1 (background hash), US-U2 (Dedup-able wired), US-U3 (= glyph),
  # US-U4 (u from main view), US-U5 (dialog applies plan), US-U6 (post-unify
  # update without restart). Demonstrable to stakeholders.
  # ===========================================================================

  @walking-skeleton @us-u1 @us-u2 @us-u3 @us-u4 @us-u5 @us-u6 @real-io @adapter-integration
  Scenario: Devon reclaims disk by unifying a duplicated model from the main view
    # AC-U1.1, AC-U2.1, AC-U2.3, AC-U3.1, AC-U3.2, AC-U4.1, AC-U4.2,
    # AC-U5.1, AC-U5.4, AC-U6.1, AC-U6.2, AC-U6.4, AC-U6.7, AC-CONS-1, AC-CONS-4
    Given Devon has Ollama installed with a single model file
    And Devon has Hugging Face cache installed with a byte-identical copy of the same model file
    And the two copies live on separate inodes on the same filesystem
    When Devon launches modeltap
    Then within 1 second Devon sees both rows in the right pane
    And while hashing is in progress the summary bar shows "Dedup-able: computing..."
    And the summary bar does NOT show "Dedup-able: 0 B" as a final value
    When background hashing completes for both copies
    Then the duplicated model row shows the dedup-able glyph "="
    And the summary bar shows "Dedup-able: <model-size>" matching the row's size
    When Devon highlights the duplicated row and presses "u" from the main view
    Then a unify dialog opens with the Hugging Face copy listed as a target, checked
    And the dialog shows "Total reclaim: <model-size>"
    When Devon presses Enter to apply the plan
    Then the two copies share one inode after the action completes
    And the model row glyph flips from "=" to "#" without Devon restarting modeltap
    And the summary bar "Dedup-able" value decreases by the model size
    And the summary bar "Unified" count increments by 1
    And launch.log records an "action.unify" event with outcome "success"

  # ===========================================================================
  # US-U1 — Background SHA256 hashing with progress
  # ===========================================================================

  @skip @us-u1 @k3-latency @real-io @adapter-integration
  Scenario: First paint completes before any hashing begins
    # AC-U1.1
    Given Devon has 19 model files distributed across 4 tools
    When Devon launches modeltap
    Then within 1 second all 19 model rows are visible in the right pane
    And every row shows the pending-hash glyph "?"
    And the status line shows "Hashing 0/19..."

  @skip @us-u1 @nfr-perf
  Scenario: Hashing-progress count advances as hashes complete
    # AC-U1.2
    Given Devon's session has 19 discovered model files and hashing has begun
    When the background hash worker has completed 7 hashes
    Then the status line shows "Hashing 7/19..."
    And 7 rows have flipped from "?" to one of "-", "=", or "#"

  @skip @us-u1 @nfr-perf
  Scenario: UI key handlers stay responsive while hashing runs in the background
    # AC-U1.3, NFR-3
    Given hashing is in progress with the status line showing "Hashing 4/19..."
    When Devon presses "j" to move the highlight down
    Then the highlighted row changes within 100 milliseconds
    And the hashing-progress count is unaffected by the keypress

  @skip @us-u1 @property @nfr-perf
  Scenario: Hashing of a typical install completes within the NFR-2 budget
    # AC-U1.4, NFR-2 — property-shaped: holds for any typical install fixture
    Given a typical install fixture (~20 files, ~50 GB total, warm SSD)
    When Devon launches modeltap and waits for hashing to finish
    Then the status line shows "Hashing complete" within 60 seconds at the 95th percentile

  @skip @us-u1 @real-io
  Scenario: Quitting during hashing exits cleanly within the shutdown budget
    # AC-U1.5, NFR-4 — no persistent state survives a quit-during-hashing
    Given hashing is in progress with completed less than total
    When Devon presses "q"
    Then modeltap exits within 500 milliseconds
    And no partial-state file is written under "${HOME}/.modeltap/"

  # ===========================================================================
  # US-U2 — Wire dedup-able bytes from classifier to summary bar
  # ===========================================================================

  @skip @us-u2 @cross-artifact
  Scenario: Summary bar shows "computing..." while any hash is pending
    # AC-U2.1, AC-U2.3 — fixes the v1 hardcoded "Dedup-able: 0 B" lie
    Given hashing is in progress and at least one hash has not yet completed
    When the summary bar paints
    Then it shows "Dedup-able: computing..."
    And it does NOT show "Dedup-able: 0 B"

  @skip @us-u2 @cross-artifact @real-io
  Scenario: Summary bar reads from the same source as the row glyphs
    # AC-U2.2, AC-U2.4, AC-CONS-1
    Given hashing is complete for every model in Devon's install
    When the summary bar shows "Dedup-able: <X>"
    Then the sum of sizes of rows displaying the glyph "=" is exactly <X>

  @skip @us-u2
  Scenario: Summary bar honestly shows "Dedup-able: 0 B" when there are no duplicates
    # AC-U2.5 — Riley's install has no cross-tool duplicates
    Given Riley has 8 distinct models with no cross-tool overlap
    When hashing completes for every model
    Then the summary bar shows "Dedup-able: 0 B"
    And the status line shows "Hashing complete"

  # ===========================================================================
  # US-U3 — Row glyph reflects dedup state
  # ===========================================================================

  @skip @us-u3 @real-io
  Scenario: A dedup-able model shows the "=" glyph in the right pane
    # AC-U3.1, AC-U3.2, AC-U3.3
    Given a model has byte-identical copies on two separate inodes across two tools
    When hashing completes and the row paints
    Then the row shows the glyph "=" in the dedup column

  @skip @us-u3 @real-io
  Scenario: An already-hardlinked model shows the "#" glyph, not "="
    # AC-U3.2 — distinguishes "dedup-able" from "already unified"
    Given a model is hardlinked between two tools (one inode, two paths)
    When hashing completes and the row paints
    Then the row shows the glyph "#" in the dedup column
    And the row does NOT show the glyph "="

  @skip @us-u3
  Scenario: A model currently being hashed shows the "~" glyph
    # AC-U3.2, AC-U3.4 — reactive update; no manual refresh needed
    Given the background hash worker is currently computing the SHA256 for a model
    When the row paints
    Then the row shows the glyph "~" in the dedup column

  @skip @us-u3 @real-io
  Scenario: A unique model with no cross-tool peers shows the "-" glyph
    # AC-U3.2 — single-tool model with no duplicates
    Given a model exists in only one tool with no byte-identical copies elsewhere
    When hashing completes and the row paints
    Then the row shows the glyph "-" in the dedup column

  @skip @us-u3
  Scenario: A model whose hash has not yet started shows the "?" glyph
    # AC-U3.2 — pre-hash state
    Given hashing has not yet begun for a model
    When the row paints
    Then the row shows the glyph "?" in the dedup column

  @skip @us-u3 @real-io
  Scenario: A model whose hash failed shows "-" plus a "!" decorator
    # AC-U3.5 — conservative-when-uncertain (BR-3)
    Given a model file cannot be read due to an I/O error
    When hashing fails for that file
    Then the row shows the glyph "-" in the dedup column
    And the row shows a "!" decorator next to the glyph
    And the status line for that row shows a hash-failure note

  # ===========================================================================
  # US-U4 — `u` from main view opens the unify dialog with mates pre-populated
  # ===========================================================================

  @skip @us-u4 @real-io
  Scenario: Pressing "u" on a "=" row opens the dialog with mates pre-populated
    # AC-U4.1, AC-U4.2 — fixes the v1 "u hotkey is a lie from main view" bug
    Given a "=" row is highlighted in the main view with copies in two other tools
    When Devon presses "u"
    Then the unify dialog opens
    And the dialog selects a canonical copy automatically
    And the dialog lists the two other tools' copies as targets, both checked

  @skip @us-u4 @real-io
  Scenario: Pressing "u" on a "#" row opens the dialog in informational mode
    # AC-U4.3 — already-unified model
    Given a "#" row is highlighted in the main view (already shared between two tools)
    When Devon presses "u"
    Then the unify dialog opens in informational mode
    And the dialog states the model is already unified across the two tools

  @skip @us-u4
  Scenario: Pressing "u" on a "-" row shows a status hint and does not open a dialog
    # AC-U4.4 — unique model, nothing to unify
    Given a "-" row is highlighted in the main view (unique model)
    When Devon presses "u"
    Then no dialog opens
    And the status line shows that the model is unique with no copies in other tools

  @skip @us-u4
  Scenario: Pressing "u" on a "?" row shows a "still computing" hint and does not open a dialog
    # AC-U4.5 — hash not yet computed
    Given a "?" row is highlighted in the main view (hash still pending)
    When Devon presses "u"
    Then no dialog opens
    And the status line shows that the hash is still computing and to try again in a moment

  @skip @us-u4 @real-io
  Scenario: Pressing "u" on the Detail screen still opens the unify dialog (no v1 regression)
    # AC-U4.6 — preserves the v1 behavior so existing users are not surprised
    Given Devon has opened the Detail screen for a duplicated model
    When Devon presses "u" on the Detail screen
    Then the unify dialog opens with the model's mates pre-populated

  # ===========================================================================
  # US-U5 — Unify dialog shows concrete reclaim preview and applies plan
  # ===========================================================================

  @skip @us-u5 @real-io
  Scenario: Dialog body shows canonical, per-target rows with savings, and total reclaim
    # AC-U5.1
    Given the unify dialog is open for a duplicated model with two targets
    When the dialog body paints
    Then it shows the model name and SHA256 prefix
    And it shows the canonical tool's full path
    And each target row shows the tool, the full path, the size, and the bytes saved
    And the dialog footer shows "Total reclaim: <sum-of-target-sizes>"
    And the dialog footer shows the actions "[Enter] Apply  [space] Toggle  [Esc] Cancel"

  @skip @us-u5
  Scenario: Toggling a target with space updates the total reclaim live
    # AC-U5.2, AC-U5.3
    Given the unify dialog has two targets checked with "Total reclaim: <full-sum>"
    When Devon navigates to one target and presses space
    Then that target's checkbox is unchecked
    And the dialog updates "Total reclaim:" to the sum of remaining checked targets

  @skip @us-u5 @real-io @kpi-instrumentation
  Scenario: Pressing Enter applies the plan and produces a hardlink
    # AC-U5.4
    Given the unify dialog is open with both targets checked
    When Devon presses Enter
    Then per-target progress lines appear in order
    And the targeted files share one inode with the canonical file
    And launch.log records exactly one "action.unify" event with outcome "success"

  @skip @us-u5
  Scenario: Pressing Esc cancels the dialog without filesystem change
    # AC-U5.5 — destructive action requires explicit confirmation
    Given the unify dialog is open with both targets checked
    When Devon presses Esc
    Then the dialog closes
    And no inode merge has occurred
    And the row glyph remains "="

  @skip @us-u5 @real-io
  Scenario: Cross-filesystem fallback dialog appears when a target is on a different filesystem (ADR-008)
    # AC-U5.6 — preserves the v1 cross-fs choice path
    Given the unify dialog is open and one target lives on a different filesystem
    When Devon presses Enter
    Then the cross-filesystem fallback dialog appears with [s]kip / [c]opy / [x]cancel options

  # ===========================================================================
  # US-U6 — Post-unify row glyph and summary bar update without restart
  # ===========================================================================

  @skip @us-u6 @real-io @cross-artifact
  Scenario: Successful full unify flips glyph "=" to "#" and updates the summary bar
    # AC-U6.1, AC-U6.2, AC-U6.4, AC-U6.6, AC-U6.7
    Given Devon has just successfully unified a model into one inode across all its targets
    When the unify-completion event is processed by the TUI
    Then within 200 milliseconds the model's row glyph is "#"
    And the summary bar "Dedup-able" value has decreased by the reclaimed bytes
    And the summary bar "Unified" count has incremented by 1
    And Devon has not restarted modeltap

  @skip @us-u6
  Scenario: Summary bar shows transient "(was X)" delta then collapses after five seconds
    # AC-U6.5
    Given a unify just completed reclaiming a known number of bytes
    When the summary bar paints immediately after the action
    Then it shows the new "Dedup-able" value followed by "(was <previous-value>)"
    When five seconds pass
    Then the summary bar shows the new "Dedup-able" value without the "(was ...)" annotation

  @skip @us-u6 @real-io
  Scenario: Partial-success unify leaves the row glyph as "="
    # AC-U6.3, AC-U6.6 — only some targets succeeded; not fully unified
    Given Devon attempts to unify a model with two targets
    And one target succeeds and the other fails with permission denied
    When the unify-completion event is processed
    Then the model's row glyph remains "="
    And the summary bar "Unified" count does NOT increment
    And the summary bar "Dedup-able" value decreases only by the bytes of the successful target

  # ===========================================================================
  # US-U7 — `[All Unified]` pseudo-tool slot in left pane
  # ===========================================================================

  @skip @us-u7
  Scenario: The "[All Unified]" slot appears below the real tool slots in the left pane
    # AC-U7.1, AC-U7.2 (badge)
    Given Devon launches modeltap with four tools configured
    When the left pane paints after hashing completes
    Then the left pane lists the four real tool slots
    And below them the left pane lists a slot labeled "[All Unified]" with a count badge

  @skip @us-u7 @cross-artifact
  Scenario: Selecting "[All Unified]" filters the right pane to "#" rows only
    # AC-U7.3, AC-U7.6
    Given Devon's session has five models with the "#" glyph
    When Devon navigates to "[All Unified]" in the left pane
    Then the right pane shows exactly five rows
    And every visible row corresponds to a model with the "#" glyph

  @skip @us-u7 @cross-artifact
  Scenario: "[All Unified]" row format includes name, size, tool count, and savings
    # AC-U7.4
    Given the "[All Unified]" view is shown
    When Devon reads a row for a model that is unified across three tools at 4.7 GB
    Then the row shows the model name
    And the row shows "4.7 GB"
    And the row shows "3 tools"
    And the row shows "saves 9.4 GB"

  @skip @us-u7 @cross-artifact
  Scenario: "[All Unified]" footer aggregates total models unified and total bytes reclaimed
    # AC-U7.5
    Given the "[All Unified]" view shows five unified models
    When the right-pane footer paints
    Then the footer shows "Unified: 5 models | Total reclaimed by unification: <sum-of-saves>"
    And the sum equals the sum of the "saves" column across the five rows

  @skip @us-u7 @cross-artifact
  Scenario: "[All Unified]" badge, summary-bar count, and right-pane row count all agree
    # AC-U7.6, AC-CONS-2 — single source of truth across UI surfaces
    Given the "[All Unified]" badge shows "(5)"
    Then the summary bar shows "Unified: 5 models"
    And selecting "[All Unified]" shows exactly 5 rows in the right pane

  # ===========================================================================
  # US-U8 — `[All Unified]` empty state with onboarding guidance (P2 polish)
  # ===========================================================================

  @skip @us-u8 @release-2
  Scenario: Empty-state guidance is shown when no models are unified yet
    # AC-U8.1, AC-U8.3
    Given Devon's install has zero unified models and hashing is complete
    When Devon navigates to "[All Unified]"
    Then the right pane shows guidance text inviting Devon to find a "=" row and press "u"

  @skip @us-u8 @release-2
  Scenario: Hashing-in-progress empty state is distinct from the truly-empty empty state
    # AC-U8.2 — honest UI: don't show "no models" when we don't know yet
    Given hashing is in progress and the unified count is unknown
    When Devon navigates to "[All Unified]"
    Then the right pane shows a "Hashing in progress" message instead of the onboarding text

  # ===========================================================================
  # US-U9 — Detail screen for unified model shows shared inode and paths (P2)
  # ===========================================================================

  @skip @us-u9 @release-2 @real-io
  Scenario: Detail for a "#" model shows the shared inode and grouped paths
    # AC-U9.1
    Given a model is unified across three tools (one inode, three paths)
    When Devon opens the Detail screen for that model
    Then the Detail screen shows the inode number labeled "shared"
    And the Detail screen lists the three paths grouped under the same inode
    And the Detail screen shows "Saves vs. separate copies: <savings>"

  @skip @us-u9 @release-2 @real-io
  Scenario: Detail for a "=" model groups paths by inode (one group per separate copy)
    # AC-U9.2
    Given a model has three byte-identical copies on three separate inodes
    When Devon opens the Detail screen for that model
    Then the Detail screen shows three inode groups
    And each inode group lists the paths that share that inode

  @skip @us-u9 @release-2
  Scenario: Detail handles a filesystem that does not expose useful inode info
    # AC-U9.4 — graceful degradation
    Given a filesystem does not expose useful inode numbers
    When Devon opens the Detail screen for a model on that filesystem
    Then the Detail screen shows "inode: <not available on this filesystem>"
    And no crash occurs

  # ===========================================================================
  # US-U10 — Partial-success reporting (per-target outcome in toast) (P2)
  # ===========================================================================

  @skip @us-u10 @release-2 @kpi-instrumentation
  Scenario: Partial-success toast lists each target's outcome inline
    # AC-U10.1, AC-U10.2, AC-U10.5
    Given a unify completes with one target OK and one target failed with "Permission denied"
    When the toast paints
    Then the toast shows the model name and "1 of 2"
    And the toast shows the OK target's name with the bytes saved
    And the toast shows the failed target's name with reason "Permission denied"
    And the toast shows the total reclaim equal to the sum of OK targets
    And the toast points Devon to "~/.modeltap/launch.log" for full detail

  @skip @us-u10 @release-2 @real-io
  Scenario: Pressing "r" on the partial-success toast retries only the failed targets
    # AC-U10.3, AC-U10.4
    Given the partial-success toast is shown with one failed target
    When Devon presses "r"
    Then a new unify is attempted only for the failed target
    And the previously-successful target is not touched

  @skip @us-u10 @release-2
  Scenario: Total-failure toast shows zero reclaim and the row glyph stays "="
    # AC-U10.1, AC-U10.2 with all targets failed
    Given a unify attempt fails for every target
    When the toast paints
    Then the toast shows the model name and "0 of <N>"
    And the toast shows total reclaim of 0 bytes
    And the model's row glyph remains "="

  # ===========================================================================
  # Cross-artifact consistency (single-source-of-truth invariants)
  #
  # These scenarios codify the "Cross-Artifact Consistency Tests" listed in
  # shared-artifacts-registry.md. They are the v1-bug regression net: every
  # one of them is a place where the v1 code violated the invariant.
  # ===========================================================================

  @skip @cross-artifact @property
  Scenario: Pane-switch invariance — same model shows same glyph regardless of left-pane selection
    # AC-CONS-3 — the classifier output is single-source for every render path
    Given a model exists in two tools with the same dedup state
    When Devon selects the first tool's slot in the left pane
    Then the model's row in the right pane shows a particular glyph
    When Devon selects the second tool's slot in the left pane
    Then the same model's row in the right pane shows the SAME glyph

  @skip @cross-artifact @property
  Scenario: Hashing-progress count is monotonic during a session
    # AC-CONS-5 — never goes backward
    Given hashing is running for any number of files
    When the status line paints repeatedly during the session
    Then the "Hashing N/M" completed count never decreases between paints
