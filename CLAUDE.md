# Comprehensive rules
We're building a Magic: The Gathering rules enforcement engine, and as such the rules version should be consistent. A copy of the full comprehensive rules is available at docs/CR.txt and should be referenced as the definitive answer on MTG rules. Whenever possible, provide a rules reference number (e.g. 107.4) justifying any rules-related decisions in replies to the user, and in a comment at the top of relevant functions/code sections. Whenever you do provide a rules reference number in a lasting way (e.g. in code comments, in a spec document), ALWAYS grep the rules document at docs/CR.txt for the reference number (e.g. `grep '^103\\.2\\.' docs/CR.txt`) and verify that the reference actually relates to the rule that you think it does.
The format of CR reference numbers is `NNN.MMx`, where NNN is the rule number, MM is the subrule number, and x is either a full stop (`.`) or a lowercase letter. The number of digits may vary. Note that reference numbers which do have a lowercase letter and the end _do not_ have a full stop as well.
References should be included in parentheses, e.g. `(123.45b)`. Sometimes you may see the letters CR included in the parentheses; this is unnecessary, and in new references should only be done rarely, where it may be unclear or ambiguous that it is a CR reference.

# docs/todo.md

This file (docs/todo.md) is a running scratchpad I'll keep of bugs/issues as I find them. You should be aware of this file; you don't always have to refer to it as the source of what to do, but you should be aware of its contents, and delete items (bulleted list items) as they are completed. If you do delete something from this file, be sure to tell me that you've deleted it and why.

# Ensure that linter is clean before finalising a block of work

Before finishing a block of work, ensure that `cargo clippy --all-targets` output is clean, and fix anything that comes up. Use `cargo clippy --fix` as a first step. If you grep the output, be sure to include all output levels in the filter (e.g. error, warning, etc.).

# Note on running cargo test

Default to the grep summary pattern when running `cargo test`:

```bash
cargo test 2>&1 | grep -E "^test result|FAILED|error\["
```

Only escalate to the full tee approach if there are unexpected failures and you need to see individual panic/backtrace details:

```bash
cargo test 2>&1 | tee /tmp/test_out.txt; tail -100 /tmp/test_out.txt
```

**NEVER** use a raw `tail` pipe without `tee`, and only after first trying the `grep` version.

**Why:** `tail` alone misses failure details (they appear before the summary), but the full output can overflow context. The grep pattern is lightweight and shows what matters; tee preserves everything when a deeper look is needed.

**How to apply:** Always start with the grep form. If failures appear and the grep output doesn't explain why, switch to tee for that run.
