# Binding

What this project calls the things Perpetum names. The block below is what the
engine reads; everything outside it is for people.

```perp-binding
path.requirements = .harness/requirements
path.vision       = .harness/vision.md
path.links        = .harness/links.md

out.journal = .harness/journal.jsonl
out.state   = .harness/state.md

gate.cwd     = .
gate.timeout = 300
gate.test  = npm test

budget.cycle.money   = 1.00
budget.cycle.seconds = 3600
budget.batch.money   = 0.30
budget.batch.seconds = 900

git.branch.batch = perp/c{cycle}/b{batch}
git.push         = approval
```

## The gate

Uncomment `gate.test` and give it your test command. It is the definition of
working: until it exists the harness refuses to run at all, because there is
nothing that could make anything green.

Add as many as you like — any key starting `gate.` is one, and they run in the
order written, stopping at the first red.

**Commands are run directly, not through a shell.** `python -m pytest tests -q`
is fine; `cd sub && make` is not, because `&&` arrives as an argument rather than
as shell logic. Put a sequence in a script and call the script.
