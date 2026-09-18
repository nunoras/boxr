# Feature map

One file per user-facing feature that exists today.
Each file says:

- **Reach it**: the exact command a user or agent types.
- **Drive it**: the cheapest real run that exercises the feature, and the driver that runs it.
- **End state**: what stdout, the throwaway boxr home and the evidence directory must contain when it worked.
- **Gotchas**: what wastes quota, what leaves traces behind, and what fails misleadingly.

A feature file is also the claim that the feature exists.
Do not write one for something that is still being built.
Add the file in the same ticket that adds the feature, and wire its driver into `../scripts/verify-boxr.sh`.

| feature | command surface | file |
|---|---|---|
| headless launch | `boxr --harness claude --model <m> "<prompt>"` | [headless-launch.md](headless-launch.md) |
| outcomes | `boxr outcome <id> success --note "<text>"` | [outcomes.md](outcomes.md) |
| session cost | `currency` and `prices` in `config.json`, read back by launch, `boxr show` and `boxr stats` | [session-cost.md](session-cost.md) |

Account profiles (#3), the live normalized ledger (#4) and the later adapters, interactive mode and detached sessions add their own files here when they land.