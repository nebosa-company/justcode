# Impact weights

What a requirement is *worth*. Distinct from source-trust weights, which live in
[`../requirements/weights.md`](../requirements/weights.md) and measure how much
the origin of a signal counts. Two files, two meanings (Perpetum C.5).

| Class | Weight | Definition |
|---|---|---|
| High Impact | 80 | Generates revenue, unblocks it, or closes a gap a competitor is currently exploiting. |
| Delighter | 20 | Pleases existing customers or closes a small gap. |

## Reading "revenue" for a product that has none

This harness is not sold. The honest translation, fixed here so it is not
re-argued every cycle:

- **High Impact** = the loop cannot run unattended without it, *or* a competing
  harness already has it and its absence is why someone would not use this one.
- **Delighter** = the loop runs without it and someone is mildly happier.

Under that reading almost everything in the current backlog is High Impact,
because the backlog is a v1 design for a thing that does not exist yet. That is
not a scoring failure; it means the score cannot do the ordering on its own this
cycle, and effort plus dependency order has to. Recorded in
[`batches.md`](batches.md) rather than papered over.

Expect this to correct itself by cycle 3, when the spine exists and new
requirements start being genuine improvements rather than the floor.

## Effort points (Perpetum C.4)

| S | M | L | XL |
|---|---|---|---|
| 1 | 3 | 8 | 20 |

Estimated against a session that has to write the tests too, and honestly:
optimism here is what turns a five-item batch into a five-day one.
