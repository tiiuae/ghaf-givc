# AGENTS.md

## Integration Tests

- Run long VM integration tests in the background with `nohup`, redirect output to a log file, and inspect that file later.
- Example:

```bash
nohup nix run ".#run-vm-tests" -- admin > /tmp/givc-vmtest.log 2>&1 &
```

- Check progress and failures by searching the log file, for example with `grep` or `rtk grep`.
- `run-vm-tests -l` lists the known VM integration tests.
