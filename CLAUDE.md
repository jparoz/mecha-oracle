# Comprehensive rules
We're building a Magic: The Gathering rules enforcement engine, and as such the rules version should be consistent. A copy of the full comprehensive rules is available at docs/CR.txt and should be referenced as the definitive answer on MTG rules. Whenever possible, provide a rules reference number (e.g. 107.4) justifying any rules-related decisions in replies to the user, and in a comment at the top of relevant functions/code sections.

# docs/todo.md

This file (docs/todo.md) is a running scratchpad I'll keep of bugs/issues as I find them. You should be aware of this file; you don't always have to refer to it as the source of what to do, but you should be aware of its contents, and delete items (bulleted list items) as they are completed. If you do delete something from this file, be sure to tell me that you've deleted it and why.

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
