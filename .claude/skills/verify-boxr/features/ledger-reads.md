# ledger reads

The read side of the ledger: `boxr show` folds a session's summary records into one view, `boxr export --atif` writes an ATIF-v1.8 trajectory from the normalized layer, and `boxr stats` groups the summary ledger.
`--kind` on a launch declares what the session is for, and every reader shows it.

## Sub-features

- `boxr show <id>`: `session:` with `status`, `harness`, `harnessSessionId`, `model`, `effort`, `profile`, `mode`, `kind` (and `kindSource`, `resumedFrom`, `verdict`, `verdictNote` when set), optional `git:`, token counts, cost, then `files:` with the `normalized` and `raw` paths.
- `boxr export --atif <id>`: writes `sessions/<id>/export/trajectory.atif.json` and prints `export:` with `schemaVersion: ATIF-v1.8`, `steps` and `path`; a session with no steps is a usage error.
- `boxr stats --by <dims> --since <window>`: one row per group plus currency; dims are any of `model`, `harness`, `effort`, `profile`, `kind`, `status`, `verdict`, `interrupted`, `limitHit`.
- `--kind <k>`: one of `build`, `fix`, `research`, `plan`, `review`, `chore`, `docs`, `describe`, or a key under `kinds` in `config.json`; anything else exits 2 and lists the valid kinds.

## Reach it

```
boxr --harness claude --model haiku --kind describe "reply with the single word ok"
boxr show <id>
boxr export --atif <id>
boxr stats --by model,kind --since 7d
```

## Drive it

No driver of its own; the readers ride on sessions other drivers already paid for.

| reader | proven by | evidence |
|---|---|---|
| `show` with `mode`, `kind`, `kindSource` | `detached` | `show.txt` |
| `export --atif` | `detached` | `export.txt`, `trajectory.atif.json` |
| `show` with verdict, `stats --by verdict` | `outcomes` | `show.txt`, `stats.txt` |
| `show` and `stats --by model` with cost | `session-cost` | `show.txt`, `stats.txt` |
| `show` with `resumedFrom` | `resume` | `show.txt` |

## End state

For the `detached` drive:

- `show.txt` carries `status: ok`, `mode: headless`, `kind: describe` and `kindSource: declared`.
- `export.txt` carries `schemaVersion: ATIF-v1.8` and a `path` inside the session directory.
- `trajectory.atif.json` is that file, copied out before cleanup, and contains `"ATIF-v1.8"` and the boxr session id.

## Gotchas

- `export` writes into the session directory, which cleanup deletes, so the driver copies the trajectory into the evidence directory first.
- `stats` needs both `--by` and `--since`; neither has a default.
- A model missing from the price table records `null` in `summary.jsonl`, never zero, and `show` prints it as `apiEquivalentCost: unknown`.
  With no `config.json` at all the summary still records `currency: USD`.
