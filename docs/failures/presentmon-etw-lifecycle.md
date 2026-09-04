# PresentMon ETW lifecycle

## Observation

On the development Windows host, an earlier release produced a valid PresentMon CSV. Subsequent captures could start but did not consistently flush a CSV after Minecraft exited; one attempt also reported that the `MineBench` ETW trace session was already running.

## Classification

- Category: external telemetry lifecycle
- Retryable: yes
- Requires environment fix: sometimes, when a stale ETW session remains
- Requires Adapter fix: no

## Corrective changes

- Capture stdout and stderr into the run logs.
- Detect and terminate a stale named session before capture.
- Ask PresentMon to stop an existing session when starting a new capture.
- Attempt a graceful trace stop and drain before killing the process.
- Fail closed when a valid CSV with usable frame samples is not produced.

## Remaining regression

A clean PresentMon-enabled real Minecraft run must produce a non-empty CSV and `presentmon.json` with no manual ETW cleanup. Until that passes repeatably, the 1.21.1 entry remains STANDARD.
