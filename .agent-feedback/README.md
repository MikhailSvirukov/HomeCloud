# Daily code review feedback

This host-side directory is ignored by Git and mounted read-only into all four OpenCode containers. Do not put secrets here; feedback may be quoted in a public issue or PR if explicitly requested.

## Daily files

Create a separately numbered file for every review at `reviews/YYYY-MM-DD-NN.md`, where `NN` is a two-digit sequence for that day. Use `review-template.md` as the blank template. Update the `latest-review.md` symlink to point to the newest file. From the workspace root, create the next file with:

```sh
review_date=$(date +%F)
mkdir -p agent-feedback/reviews
review_number=1
while [ -e "agent-feedback/reviews/$review_date-$(printf '%02d' "$review_number").md" ]; do
  review_number=$((review_number + 1))
done
review_file=$(printf 'agent-feedback/reviews/%s-%02d.md' "$review_date" "$review_number")
cp agent-feedback/review-template.md "$review_file"
ln -sfn "reviews/$(basename "$review_file")" agent-feedback/latest-review.md
```

Write each review in its own numbered file; do not overwrite earlier reviews from the same day. At startup, orchestrators and reviewers read every file for the newest date in `reviews/`, in ascending filename order, plus `latest-review.md` and `standing-guidance.md`. The symlink is a convenient pointer to the newest individual note. Keep older dated files as an archive. Promote repeated preferences into `standing-guidance.md` only after the owner approves them. This is context, not model training.

Use these headings in each dated file where useful:

- **Keep:** behavior or code quality to repeat.
- **Change:** what to rewrite and what missed expectations.
- **Avoid:** approaches not to repeat.
- **Applies to:** `all`, `android`, `distributed-server`, `dedicated-server`, or `test-environment`.
- **Example:** optional file/PR link or short excerpt.

Default run budget: at most 20 sequential issue/PR work items and 3600 seconds (60 minutes) per container invocation. Set `AGENT_MAX_TASKS` and `AGENT_MAX_RUN_SECONDS` in the local `.env` to change these defaults. Count an item when its issue is claimed; allow only one active issue/PR cycle at a time. The operator starts the workstream explicitly. The time limit is a soft agent checkpoint, not a forced Docker timeout. For exact line feedback, use a GitHub PR review comment. GitHub Discussions are public alternatives for general feedback.
