# serve

`boxr serve` exposes the ledger over HTTP as JSON so another machine can poll this host.
It is read-only, it binds loopback by default, and it can require a bearer token.
This feature spends no quota: it never launches a harness.

## Reach it

```
boxr serve [--bind <IP>] [--port N] [--token <secret>]
```

`--bind` takes one IPv4 or IPv6 address, not arbitrary shell text.
Binding an address that is not loopback without `--token` warns on stderr.
With `--token`, every request needs `Authorization: Bearer <secret>` before it is routed.

## Drive it

```
.claude/skills/verify-boxr/scripts/verify-boxr.sh serve
```

The driver builds `target/release/boxr`, then starts it three times on port 0 against a throwaway `BOXR_HOME` and drives each with `curl`:

1. no flags, to prove the default bind and the unauthenticated read;
2. `--bind 0.0.0.0`, to prove the warning;
3. `--token <secret>`, to prove the refusal and the authenticated read.

## End state

The listen block from the default run reads `host: 127.0.0.1`, and `GET /ps` answers 200 with a JSON body holding `running`.

The `--bind 0.0.0.0` run writes a `warning:` line to stderr naming the bind and the missing token.

The `--token` run answers 401 with `{"error":"authentication required"}` for `GET /ps`, `GET /unknown` and `POST /ps` without the header, and 200 for `GET /ps` with `Authorization: Bearer <secret>`.
The secret appears in no listen block and no stderr file.

## Gotchas

- The driver needs `curl` on `PATH` and fails the guard step when it is missing.
- `boxr serve` spawns no children, so the driver kills it by its own pid and never needs a process group.
- A port of 0 means the kernel picks the port, so every assertion reads the number `serve` printed rather than assuming one.
