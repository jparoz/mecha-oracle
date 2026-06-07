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
