---
name: boxr-prompts
description: Placeholder for the boxr prompt-improvement skill. Use when the user asks to analyze or improve their coding-agent prompts with boxr.
---

# boxr prompts

This is a placeholder skill.
`boxr skill install` ships it so the install path is real before the prompt skill lands.

The full skill reads the user's prompts through `boxr prompts`, describes their typical style per kind, proposes one specific change as a variant, measures it with `boxr eval run` and `boxr eval compare`, and reports the result.
Every claim it makes must come from CLI output.

Reinstalling through `boxr skill install` replaces this file in place once the real skill ships.
