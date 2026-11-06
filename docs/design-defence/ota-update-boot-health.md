# ota-update boot-health Design Defence

## Decision

Add a standalone top-level `ota-update boot-health` operation. It evaluates the running Ghaf A/B slot, performs configured health checks, retries unhealthy trials, and promotes a trial only after it is healthy.

The current `ghaf-boot-health` shell service is intentionally unchanged. Ghaf will switch to this operation in a later change after pinning a released updater.

## Defence

### Why a standalone operation

Post-boot validation is a boot-state transition, not image installation. Keeping it separate prevents image installation code from owning reboot-time policy and gives Ghaf a small privileged integration surface.

### Why strict `bootctl` state

The operation requires exactly one selected managed Ghaf UKI and parses its boot counter from the physical filename. Missing, malformed, or ambiguous state fails closed. Stable UKI IDs are used for `set-oneshot` and `set-default`; boot-count suffixes are never passed to bootctl.

### Why the default is committed last

The existing blessed default is the rollback target. The order is:

1. Acquire the OTA lock and read current state.
2. Run fixed LUKS, verity, mount, and service checks.
3. Bless a healthy trial.
4. Set the stable entry as default.
5. Atomically persist accepted generation.

Any failure before the final step leaves the previous default and accepted state intact.

### Why fixed checks instead of arbitrary commands

Health policy is supplied as mapper names, mountpoints, and service units. The updater constructs fixed argv lists for `cryptsetup`, `veritysetup`, `findmnt`, and `systemctl`; callers cannot inject shell commands.

### Why retry re-arms the exact entry

`LoaderEntryOneShot` is consumed when a trial starts. Re-arming the exact stable entry while tries remain preserves retry behavior without changing the persistent fallback. Once the counter reaches zero, the operation returns failure and leaves systemd-boot to select the blessed fallback.

### Why atomic accepted-generation writes

Accepted generation is rollback-sensitive state. The updater writes a mode `0600` temporary file, syncs it, renames it, and syncs the parent directory. Partial writes therefore cannot create a falsely accepted generation.

## Rejected Alternatives

- Nesting the operation under `ota-update image` couples post-boot policy to installation.
- Keeping parsing and retry orchestration in Ghaf shell duplicates security-sensitive boot-state logic.
- Accepting arbitrary health commands expands the privileged command surface and recreates shell injection risk.

## Evidence

- Rust unit tests cover healthy promotion, fallback, retry, exhausted trials, malformed counters, failed checks, and atomic state handling.
- The real EFI VM test covers trial boot parsing, health-check dry-run/failure behavior, and rollback after exhausted retries.
