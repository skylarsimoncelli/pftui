# News Topic Classifier Fixtures

This fixture set guards the rule-based news topic classifier against silent
drift. Each JSONL row is a hand-labeled article-like example with:

- `title`
- `category`
- `description`
- `expected_topic`
- `source_domain`
- `source_tier`
- optional `extra_snippets`

The expected labels use the audit buckets from `TODO.md`: `fed`, `inflation`,
`geopolitics`, `commodities`, `crypto`, `equities`, and `other`. The test maps
native pftui topics such as `fed-policy`, `oil-energy`, and `iran-hormuz` back
to these buckets before scoring.

The text is deliberately hand-written and paraphrased rather than copied from
published articles. Use real outlet domains and source tiers to preserve ingest
shape, but do not paste copyrighted article text.

When a new ingest pattern appears in `pftui data news`, add 5-10 rows:

1. Pick the expected audit bucket before checking classifier output.
2. Include the source domain and tier that produced the pattern.
3. Keep the title/description short and representative.
4. Run `cargo test topic_classifier_accuracy_floor_on_fixture_set -- --nocapture`.
5. If the row is misclassified, either improve classifier rules or correct the
   fixture label if the original label was wrong.
