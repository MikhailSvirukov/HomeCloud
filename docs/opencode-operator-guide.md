# OpenCode operator guide

This guide describes the four-service OpenCode setup in this workspace. The operator starts one workstream per daily cycle, updates that day's review, and starts the container explicitly. Containers are disposable; host checkouts and Docker named volumes hold the durable state.

## What you need to choose and enter

| Setting | Where | Needed for |
| --- | --- | --- |
| Fine-grained GitHub PAT | `.env` as `GITHUB_MCP_TOKEN` | GitHub MCP read/write operations |
| DeepSeek API key | `.env` as `DEEPSEEK_API_KEY` | Model access for every headless agent run |
| Model ID (`provider/model-id`) | `.opencode/opencode.jsonc` | Selects the model used by orchestrator and subagents |
| Default branch | `.env` as `DEFAULT_BRANCH` | Usually `main` |
| Git author name/email | `.env`, optional | Local commits made by orchestrators |
| Checkout paths | `.env` as `*_CHECKOUT` | Four independent clones under `../homecloud-clones/` |

Do not put provider keys or GitHub tokens in tracked files. `.env` is ignored by Git. The model ID is not secret and can be stored in the tracked OpenCode config. Compose passes the DeepSeek key into the OpenCode process; the tracked config reads it through environment substitution.

## One-time host setup

1. Install Docker Engine/Desktop with the Docker Compose plugin and ensure the daemon is available to the account that will run the agents.
2. The public repository is the `HomeCloud/` directory. It is already initialized: `main` tracks `origin/main`, and the initial commit contains `LICENSE` and `README.md`. The parent `backup/` directory holds the local operator setup; it is not the Git repository. Before starting agents, copy the project instructions/configuration into `HomeCloud/`, commit and push that bootstrap change, then fast-forward all four workstream clones. From `/home/mikhail/workdir/backup`, run:

   ```sh
   cp -a AGENTS.md .opencode docs contracts .gitignore .dockerignore .env.example Dockerfile.agent compose.yaml compose.test-engine.yaml HomeCloud/
   cd HomeCloud
   git add AGENTS.md .opencode docs contracts .gitignore .dockerignore .env.example Dockerfile.agent compose.yaml compose.test-engine.yaml
   git commit -m "Add initial agent and project setup"
   git push origin main
   for checkout in ../homecloud-clones/android ../homecloud-clones/distributed-server ../homecloud-clones/dedicated-server ../homecloud-clones/test-environment; do
     git -C "$checkout" pull --ff-only origin main
   done
   ```

   Do not copy `.env`, `.git/`, `agent-feedback/`, `agent-state/`, or the clone directories. The clones currently contain only the initial commit, so they do not yet have `.opencode/` or the project instructions required by the agents.
   The repository may start with no project files. Seed the initial backlog with four separate area-bootstrap issues plus narrowly scoped shared-file setup issues assigned to `dedicated-server`. Each area orchestrator can create its own root directory, `AGENTS.md`, independent workspace/build configuration, and local CI checks. The dedicated-server orchestrator owns shared root files, `.github/workflows/`, and other repository-wide setup; GitHub Actions workflow YAML lives at the repository root and calls the area-local checks.
3. From `/home/mikhail/workdir/backup`, copy the example environment file (the example already points to `/home/mikhail/workdir/homecloud-clones/<workstream>`):

   ```sh
   cp .env.example .env
   ```

4. Create a fine-grained personal access token in GitHub. Limit it to this one repository. Grant repository permissions:
   - **Contents: Read and write** — create branches and publish changed files.
   - **Issues: Read and write** — list, read, and claim issues with labels.
   - **Pull requests: Read and write** — create/read PRs and submit review comments.
   - **Actions: Read-only** — inspect CI results.
   - **Metadata: Read-only** is normally selected automatically by GitHub.

   Set a short expiry that suits your rotation schedule, then paste the token into `GITHUB_MCP_TOKEN` in `.env`. Never paste it into `.opencode/opencode.jsonc`, an issue, a review note, or a commit. GitHub documents the repository permission model in its [fine-grained token permission reference](https://docs.github.com/en/rest/authentication/permissions-required-for-fine-grained-personal-access-tokens).
Use this single GitHub account for every OpenCode role. The existing `GITHUB_MCP_TOKEN` needs **Pull requests: Read and write** to create PRs and write review comments. Since the same account authors and reviews PRs, GitHub will not accept a formal approval from the reviewer role. The reviewer records its internal result in a `COMMENT` review using `[agent-review: changes-requested]` or `[agent-review: approved]`; branch protection requiring approval by a different person cannot be satisfied automatically in this setup.
5. The active `.env` and `.env.example` checkout paths match the existing `homecloud-clones/` directory. Confirm the four `*_CHECKOUT` values in `.env` are `../homecloud-clones/android`, `../homecloud-clones/distributed-server`, `../homecloud-clones/dedicated-server`, and `../homecloud-clones/test-environment`. Set `DEFAULT_BRANCH` if the repository does not use `main`. `GIT_AUTHOR_NAME` and `GIT_AUTHOR_EMAIL` are optional; Compose supplies `HomeCloud Agent` and a noreply address when they are blank.
6. The four clones already exist under `/home/mikhail/workdir/homecloud-clones/`. Each must have the same `origin`, track `main`, and remain a separate checkout. If recreating them, use:

   ```sh
   mkdir -p ../homecloud-clones
   git clone <PUBLIC_REPOSITORY_URL> ../homecloud-clones/android
   git clone <PUBLIC_REPOSITORY_URL> ../homecloud-clones/distributed-server
   git clone <PUBLIC_REPOSITORY_URL> ../homecloud-clones/dedicated-server
   git clone <PUBLIC_REPOSITORY_URL> ../homecloud-clones/test-environment
   ```

   Each clone must have the same `origin` and default branch. Never mount one clone into two services.
7. Create the repository labels once in GitHub: open the repository’s **Issues** tab, choose **Labels**, then **New label**. These are repository-level labels visible on both issues and pull requests, not labels tied to one issue. Create: `area:android`, `area:distributed-server`, `area:dedicated-server`, `area:test-environment`, `agent:claimed/android`, `agent:claimed/distributed-server`, `agent:claimed/dedicated-server`, `agent:claimed/test-environment`, and `agent:do-not-pick`. GitHub documents that repository labels can be applied to issues and PRs in [Managing labels](https://docs.github.com/en/issues/using-labels-and-milestones-to-track-work/managing-labels).
8. Create a dated review file and point `latest-review.md` at it, as described below.
9. Build the image:

   ```sh
   docker compose build
   ```

   The role images are defined in `Dockerfile.agent`: Android gets JDK 17 and Android SDK 36, Rust services get Rust 1.99.0 plus liburing/OpenSSL build dependencies, and the test image additionally gets Docker CLI/Compose. The host still needs Docker to build/run them. Agent containers do not execute test suites locally; GitHub Actions on the pull request is the test authority. Android connected tests require an external emulator/device. Testcontainers uses the isolated DinD service in `compose.test-engine.yaml`; this optional test-only service requires privileged mode.

## Where OpenCode agent settings live

The project-level OpenCode configuration is [`.opencode/opencode.jsonc`](../.opencode/opencode.jsonc). It defines orchestrator/subagent roles, tool permissions, MCP setup, the shared model, and environment-based DeepSeek authentication. Workstream-specific role instructions live in [`.opencode/roles/`](../.opencode/roles/); global behavior and area briefs live in `docs/agent-global.md` and `docs/agent-*.md`. Repository coding instructions belong in root or area-local `AGENTS.md`. The API key stays in the ignored `.env` file and is passed to OpenCode at runtime.

The files currently in `/home/mikhail/workdir/backup/.opencode/` are only visible to the containers after they are committed and pushed into the `HomeCloud` repository and the four clones are updated, because `/workspace` is mounted from those clones.

## GitHub MCP and model credentials

### GitHub MCP

The MCP connections are declared in [`.opencode/opencode.jsonc`](../.opencode/opencode.jsonc). The `github` connection uses your one GitHub token and advertises only the required tools. OpenCode prefixes tool names with the server name, so workers create PRs with `github_create_pull_request`. Per-agent permissions grant the orchestrator merged-PR feedback reading and merge, workers PR creation/read, and the reviewer PR review/comment submission.

The Remote GitHub MCP endpoint requires a bearer access token. GitHub's server documentation says the remote endpoint does not provide the OAuth login service and does not support dynamic client registration, so this setup uses fine-grained PATs rather than relying on OpenCode's generic dynamic OAuth flow. The token comes from `.env` and is used as the `Authorization` header; it is not written into tracked config. GitHub does not accept an official approval from the same account that authored the PR, so the reviewer role uses a `COMMENT` review with an internal status marker instead.

There is an important security limit: the OpenCode process and shell/test commands run as the same container user. A command or project test could inspect that process environment and obtain the GitHub PAT or the DeepSeek API key. Role permissions limit model-invoked MCP tools, but neither is an operating-system boundary against malicious code. Use this only with trusted repository code. A credential-free build runner plus a separate GitHub MCP credential broker would be needed before testing untrusted changes.

### Model provider and model

The four services run OpenCode through `opencode run`, with no TUI or interactive prompt. Set `DEEPSEEK_API_KEY` in `.env`; Compose passes it to the process and `.opencode/opencode.jsonc` uses it for the configured `deepseek/deepseek-flash` model. You do not need to run `/connect` in each service. The GitHub MCP connection is initialized from the `GITHUB_MCP_TOKEN` environment variable.

The current shared model is `deepseek/deepseek-flash`. To change it, edit the top-level `model` field in `.opencode/opencode.jsonc` to a model ID supported by the selected provider. OpenCode's [model guide](https://dev.opencode.ai/v2/docs/models/) documents model IDs and selection.

## Daily review files

Keep a permanent archive under `agent-feedback/reviews/`, with one numbered file per review: `YYYY-MM-DD-NN.md`. This directory is host-side, ignored by Git, and mounted read-only at `/workspace/.agent-feedback` in each service. At startup, orchestrators and reviewers read all files for the newest date in ascending filename order, plus `latest-review.md` and `standing-guidance.md`.

For each new day, from `/home/mikhail/workdir/backup`:

```sh
review_date=$(date +%F)
review_number=1
while [ -e "agent-feedback/reviews/$review_date-$(printf '%02d' "$review_number").md" ]; do
  review_number=$((review_number + 1))
done
review_file=$(printf 'agent-feedback/reviews/%s-%02d.md' "$review_date" "$review_number")
cp agent-feedback/review-template.md "$review_file"
ln -sfn "reviews/$(basename "$review_file")" agent-feedback/latest-review.md
```

Fill each new numbered file with that review's feedback; do not overwrite earlier notes from the same day. Keep all dated files after the cycle. Put recurring rules in `standing-guidance.md` only after you approve them. Exact line-level comments can still go directly on a GitHub PR.

## Start a daily work cycle

Start only the workstream you want to review that day. For example, Android:

```sh
docker compose run --rm android
```

Other service names are `distributed-server`, `dedicated-server`, and `test-environment`. Each service runs `opencode run --continue --agent orchestrator --auto` using its own named data volume, so it resumes that workstream's latest saved OpenCode session. The run reads the latest daily review, resumes the issue in `.agent-state/project-state.md` if there is one, checks the open GitHub issue queue, and claims a clearly matching issue. It creates a new issue only if no suitable owner-created issue exists. The orchestrator resolves ambiguity from the approved requirements, prior decisions, v1 scope, and platform/security guidance; it splits cross-workstream work by ownership and routes shared paths to `dedicated-server`. It documents any required SRS decision in its own PR and proceeds without waiting for operator approval. OpenCode runs without the TUI; progress and the final summary appear in the `docker compose` output.

Defaults are 20 issue/PR work items and 3600 seconds (60 minutes) per invocation. Set `AGENT_MAX_TASKS` and `AGENT_MAX_RUN_SECONDS` in `.env` to change them; for example, `AGENT_MAX_TASKS=3` and `AGENT_MAX_RUN_SECONDS=1800` caps a run at three items and 30 minutes. The task limit is counted when an issue is claimed. The time limit is a soft model-instructed checkpoint, not a hard Docker timeout. After each merge the orchestrator records state and may pick up the next clear issue while both limits allow. On a safe checkpoint it records the issue, branch, progress, and blocker in the workstream state file. `docker compose run --rm` removes the run container when OpenCode exits, while the checkout and separate named volumes preserve workstream state and session history. Run a new container after updating the review; the resumed session reads all notes for the newest date. It does not watch GitHub or the review directory in the background.

Do not use `docker compose up -d` for this operator workflow: that starts a long-running service container. Avoid `docker compose down -v` unless you intend to delete the OpenCode named volumes, including session history and any credentials added interactively in the future.

If a previously created OpenCode volume is not writable by UID 1000, repair that service's three OpenCode volumes once. For the current Android error, run:

```sh
sudo docker compose run --rm --user 0:0 --cap-add CHOWN --entrypoint sh android -c \
  'chown -R 1000:1000 /home/opencode/.local/share/opencode /home/opencode/.config/opencode /home/opencode/.local/state/opencode'
```

Repeat with the affected service name if another workstream has the same problem. This changes ownership only inside that service's named OpenCode volumes; it does not expose or modify host files.


### Testcontainers Docker Engine

Docker-backed tests in GitHub Actions use the runner's Docker service. Agent containers do not run Docker-backed tests locally and do not mount the host Docker socket. The optional Docker-in-Docker setup in `compose.test-engine.yaml` remains available for explicit manual test-environment work; it serves the test agent over TLS and requires Docker's `privileged` mode. Do not use it during normal agent runs.

Start the self-contained test setup with:

```sh
docker compose -f compose.yaml -f compose.test-engine.yaml build test-environment
docker compose -f compose.yaml -f compose.test-engine.yaml run --rm test-environment
```

After exiting OpenCode, stop and remove the DinD service while preserving its named image/data cache:

```sh
docker compose -f compose.yaml -f compose.test-engine.yaml down
```

The per-service OpenCode volumes and the `test-docker-data` named volume persist. To delete the Docker test cache too, use `down -v` only when you intend to remove those volumes. Do not mount `/var/run/docker.sock` from the host into any agent container.

## What each container can access on the host

Each service has its own full clone of the same monorepo, mounted read/write at `/workspace`. All four clones use the same Git author/committer identity. Each clone contains the shared global instructions and contracts; agents are instructed to work only in their assigned top-level area:

- Android: `../homecloud-clones/android`
- Distributed storage: `../homecloud-clones/distributed-server`
- Dedicated server: `../homecloud-clones/dedicated-server`
- Test environment: `../homecloud-clones/test-environment`

The containers do not get general access to `/home/mikhail` or the rest of the host filesystem. They have these additional mounts:

| Container path | Host/Docker storage | Access | Contents |
| --- | --- | --- | --- |
| `/workspace` | Its one clone directory | Read/write | Full public monorepo clone; policy scope is one workstream area plus shared instructions/contracts |
| `/workspace/.agent-feedback` | `/home/mikhail/workdir/backup/agent-feedback` | Read-only | Daily review archive, latest pointer, standing guidance |
| `/workspace/.agent-state` | `/home/mikhail/workdir/backup/agent-state/<workstream>` | Read/write | Per-workstream resume state |
| `/home/opencode/.local/share/opencode` | Per-service named volume | Read/write | OpenCode sessions, logs, and provider/MCP state |
| `/home/opencode/.config/opencode` | Per-service named volume | Read/write | User-level OpenCode config |
| `/home/opencode/.local/state/opencode` | Per-service named volume | Read/write | OpenCode runtime state |
| Cargo, Rust, Gradle, Android and general caches | Per-service named volumes | Read/write | Downloaded toolchains and dependencies |
| `/tmp` | Container tmpfs | Read/write, ephemeral | Temporary files; lost when the container exits |

The per-service named volumes survive `docker compose run --rm`; the checkout and `agent-state` are host bind mounts and also survive. `docker compose down -v` deletes the named volumes. The containers run as UID/GID 1000, with a read-only root filesystem, dropped Linux capabilities, isolated per-service Docker networks, and no host Docker socket. They can still access the public Internet for model, GitHub, and package downloads.

## Current readiness gaps

The control directory `backup/` is not a Git repository; the public repo is `HomeCloud/` and the four clones are under `homecloud-clones/`. Docker CLI 29.9.0 is present, but daemon access depends on the operator account's Docker permissions. Android connected tests still need an emulator or physical device. Testcontainers uses the optional privileged DinD service described above; it does not mount the host Docker socket.
