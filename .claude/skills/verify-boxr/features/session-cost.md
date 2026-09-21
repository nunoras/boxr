# session cost

Every session summary records an API-equivalent cost plus the configured currency, and `boxr stats` sums and groups that cost.

## Reach it

Put a price table and a currency in `config.json` under the boxr home:

```json
{"currency":"USD","prices":{"haiku":{"input":2.0,"output":6.0,"cached":0.3,"reasoning":60.0}}}
```

Then launch a session on a model the table names:

```
boxr --harness claude --model haiku "reply with the single word ok"
```

The launch output carries `apiEquivalentCost` and `currency` in its `ledger:` section.
`boxr show <id>` reads the same pair back out of the summary, and `boxr stats --by model` sums the cost and counts the sessions the table does not price.

## Drive it

```
.claude/skills/verify-boxr/scripts/verify-boxr.sh session-cost
```

That is the headless-launch drive plus a price table.
The driver writes the table above into the throwaway home before the launch, so the one real session it starts is priced, and it recomputes the cost from the token counts that session recorded.

## End state

stdout is TOON whose `ledger:` section carries a positive `apiEquivalentCost`, `currency: USD` and no `costError`.
The summary line the throwaway home accumulates under `summary.jsonl` records the same cost at full precision, equal to

```
((prompt - cached) * input + cached * cachedRate + (completion - reasoning) * output + reasoning * reasoningRate) / 1e6
```

over the token counts that same line records, and `currency: "USD"`.

`boxr show <id>` prints the cost and currency the launch printed, and `boxr stats --by model --since 1d` prints one row `haiku,USD,1,...` whose `apiEquivalentCost` cell equals the launch output and whose `unpricedSessions` cell is `0`.

The evidence directory keeps `launch.txt`, `show.txt`, `stats.txt`, `summary.jsonl`, `meta.txt` and `sessions/<id>/`.
`meta.txt` also records the price table the run used as `configJson`.

## Gotchas

- The model key in the price table must match the summary's `model` exactly.
  For the claude harness that is the `--model` value verbatim, so the driver prices `haiku`.
- A recorded `costError` with a null cost means the arithmetic failed, not that the model is missing from the table.
  The driver fails when the launch reports one.
- Reasoning tokens are a subset of completion tokens and are billed at the reasoning rate, which maps whatever split the harness reports (Claude's `thinking_tokens`, pi's `reasoning`); a provider that bills thinking as ordinary output needs the reasoning rate set equal to the output rate.
  A one-line low-effort run usually records none, so the driver's table puts the reasoning rate ten times the output rate to keep the arithmetic discriminating either way.
  The black-box proof of that rate is `tests/pi.rs` and `tests/headless.rs`.
- Only one session is driven, because every drive spends quota.
  Currency switching and the unpriced-model rule each need another paid session, so the driver leaves both to the black-box suite in `tests/headless.rs`.
