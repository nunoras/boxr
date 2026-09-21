# models and list

`boxr models --harness pi` prints the model ids the installed pi catalog advertises, and `boxr list` reads the folded summary ledger without starting anything.

## Reach it

```
boxr models --harness pi
boxr list
boxr list --all
boxr list --limit 5
```

`boxr models` runs the harness's own discovery command, so it never spends quota and never keeps a hardcoded model list.
A harness that cannot list its models is a usage error: `boxr models --harness claude` reports that claude does not expose a catalog.

For pi, a fresh launch checks `--model` against that catalog before it allocates a session, so an unknown id exits 2, names the closest full id and creates nothing:

```
boxr --harness pi --model xai/grok-9.9 "say ok"
```

## Drive it

```
.claude/skills/verify-boxr/scripts/verify-boxr.sh models-and-list
```

The driver builds `target/release/boxr` and then runs, from a scratch directory next to the throwaway home:

```
BOXR_HOME=<throwaway>/home boxr models --harness pi
BOXR_HOME=<throwaway>/home boxr models --harness claude
BOXR_HOME=<throwaway>/home boxr list
```

It launches no model, so it spends no quota.

## End state

`models.txt` holds TOON with a `models[N]{provider,model}:` table whose rows are `provider,model` pairs from `pi --list-models`.
`unsupported.txt` holds the usage error for claude and names the missing catalog.
`list.txt` holds a `sessions[0]{id,state,harness,model,status,start,durationMs,kind,verdict}:` table, empty because the throwaway home has no sessions.

## Gotchas

- `pi --list-models` reads the installed catalog and the ambient pi config, so the count depends on what pi is logged into and which catalogs it has updated.
- `boxr list` is read-only: it folds `summary.jsonl` and adds running launch records, so it never reconciles a dead supervisor the way `boxr ps` does.
- The default view is 20 rows. `--all` lifts that unless `--limit` is also given.
