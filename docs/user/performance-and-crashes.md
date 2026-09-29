# Performance and crashes

## CPU, memory, TPS and MSPT

The Overview shows CPU and memory of the server process (from Windows) and — for
supported software — **TPS** and **MSPT** from the server itself:

| Software | Source |
|---|---|
| Paper, Purpur | `tps` and `mspt` commands (TPS is the server's 1-minute average) |
| Vanilla and mod loaders, Minecraft 1.20.3+ | `tick query`; vanilla reports no TPS, so MCPanel shows `min(target rate, 1000 ÷ MSPT)`, labelled *calculated* |

MCPanel asks every 15 seconds while the server runs. The replies are hidden from the
console but appear in the server's own log file; turn this off in Settings (*Collect TPS
and MSPT*). A server that does not answer is not asked again until it restarts. MCPanel
never estimates values it cannot read.

## Automatic restart

The server's Settings tab has a restart policy (default: on, up to 3 restarts within 10
minutes, starting after 10 seconds and doubling). Problems a restart cannot fix — wrong
Java, EULA not accepted, port in use, invalid JVM option — are never restarted. Optionally
MCPanel backs the server up before restarting.

## Crash analysis

Each crash in *Recent crashes* shows what happened (out of memory, watchdog, crash report,
…), the root-cause exception when there is one, and the **plugins or mods that ran in the
crashing code**. MCPanel finds them from the jar named in the stack trace (Paper) or from
class packages that belong to exactly one installed jar. Check such a plugin/mod for an
update, or disable it to test.

## Diagnostics for getting help

**Diagnostics** (Overview → Details) saves a ZIP with the server's latest log and crash
reports, MCPanel's console captures and logs, the plugin/mod list, versions and a
`server.properties` with secrets removed. Worlds and sensitive files are never included.
Logs can contain player names and IP addresses — share the file only with people you trust.
