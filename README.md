# gyat

A small, Git-like version control system. One binary for the client, one small
server binary, no daemon, no URLs to remember.

```bash
gyat add .
gyat commit "first version"
gyat push            # to your server, configured once
```

## Install

```bash
cargo build --release
cp target/release/gyat ~/.local/bin/gyat
```

Server side:

```bash
./scripts/deploy-server.sh          # builds, uploads, installs, verifies
```

That installs `gyat-server` to `~/.local/bin/gyat-server` on the configured box.

## Configure once

```bash
gyat setup --host 100.81.91.113 --user water
```

Writes `~/.gyatconfig.toml`:

```toml
[server]
host = "100.81.91.113"
user = "water"
base = "gyat-server-data"
key = "~/.ssh/id_ed25519"
bin = "gyat-server"
```

Every repo leaves `server = ""`, meaning "use the static box". After that you
never type a URL:

```bash
gyat push
gyat pull
gyat clone myrepo
gyat list                # repos on the server
gyat list myrepo         # branches of one repo
gyat doctor              # check the whole chain
```

## Commands

| | |
|---|---|
| `init` | create a repo (prompts, or `--name/--user/--server`) |
| `add <files...>` | stage changes; `add .` stages everything not ignored |
| `ignore` / `ignore-add <pattern>` | view / add ignore patterns |
| `commit "msg"` / `commit -m msg` | snapshot, compress, compute deltas |
| `status` | staged, unstaged, untracked |
| `log [--oneline]` | history |
| `branch [name]` | list or create |
| `branch -d <name>` / `-D` / `-r <new>` | delete (refuses unmerged) / force / rename |
| `travel <rev>` (aliases `goto`, `go`) | switch branches or commits |
| `merge <branch>` | fast-forward, merge, or conflict markers |
| `push [--force]` | publish to the server |
| `pull [<rev>]` | fetch and update the working tree |
| `clone <source> [dest]` | from the server or a local path |
| `snip top \| bottom \| range A..B \| commit --start --end` | rewrite history |
| `list` / `doctor` / `setup` | browse, diagnose, configure |

### Revisions

`travel` and `snip` take git-style revisions:

```
HEAD~2      two commits back
HEAD^       first parent (same as HEAD~1)
HEAD^2      a merge commit's second parent
HEAD^0      the commit itself
main~1      one back from a branch
@           HEAD
4a609b4~1   four back from an abbreviated hash
```

### Ignores

`.gyatignore` in the repo root, plus an `[ignore] files = [...]` list in
`.gyt/config.toml`. `node_modules/`, `target/`, `.git/` and `.gyt/` are ignored
by default.

## How it stores things

Each commit keeps a **full gzipped snapshot** of the tree, so any commit can be
checked out on its own — there is no delta chain to break. Unchanged files are
shared between a commit and its parent as hardlinks, so this stays cheap:
a 42-commit repo of 2000 files costs ~18 MB, not 339 MB.

On the server:

```
/home/water/gyat-server-data/<repo>/
├── refs/heads/<branch>              # a hash, 16 bytes
└── commits/<hash>/
    ├── meta.toml                    # message, author, time, parents
    ├── snapshot/                    # your files, gzipped
    └── deltas/                      # diff against the parent
```

The path comes from `base` in `~/.gyatconfig.toml`, resolved against the SSH
login directory. There is no server-side config file or index — the path travels
with each command, which is how `gyat list` sees everything.

## Transport

One-shot commands over SSH, like `git push` → `git-receive-pack`. **There is no
HTTP server and nothing listens on a port**, so `curl` will not work — use
`gyat list` or `gyat doctor` instead.

Pushes are fast-forward gated (non-divergent updates are rejected unless
`--force`) and only send the commits the server is missing.

## Durability and safety

- refs, `HEAD`, `meta.toml` and snapshots are written to a temp file, fsynced,
  then renamed, so a crash leaves either the old file or the complete new one
- concurrent pushes to one repo are serialised with an advisory lock
- bundles reject `../` and absolute paths; shell metacharacters in names are
  quoted, never interpolated
- merges refuse to overwrite uncommitted work, and binary files are compared as
  bytes rather than merged line-by-line

## Known gaps

- `merge` has no `--abort`; finish the merge by resolving and committing
- no tags, stash, `fetch`, or reflog (`HEAD@{1}`)
- only gzip; `compression.algorithm` accepts `"none"`, anything else means gzip
- `[chunks]` is reserved and unused — chunked delta storage is not implemented
- the server has no `HEAD`, so it does not record a default branch
