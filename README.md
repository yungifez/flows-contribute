# flows-contribute

Help the Flows project with the AI usage that you already pay for. You do not need to write code.

Flows collects labels that describe what web application code does, for example "this endpoint accepts an invitation" or "this endpoint only shows a page". `flows-contribute` gets a batch of code locations from the Flows work queue. Your coding agent (Codex or Claude Code) reads the code and labels each location. The client then sends the labels back.

## What happens on your computer

1. The client downloads public open-source repositories from GitHub, at fixed commits, into `~/FlowsWork`.
2. It starts your agent on one batch, with instructions and a codebook from Flows.
3. The agent reads the code and returns one label for each location.
4. The client checks the labels, removes text that looks like a password or key, and sends them.
5. It repeats until your usage limit or the daily batch limit. Then it stops. Run it again later to continue.

The client sends only the labels, the token counts and a fingerprint (SHA-256) of the agent's log. The full agent logs stay in `~/FlowsWork/logs`.

## Safety

- **Nothing from the downloaded code runs.** The client only unpacks regular files and folders. It skips links and refuses archives with unsafe paths.
- **Codex** runs in its read-only sandbox, with no network for the commands it runs.
- **Claude Code** runs in restricted mode with only its read tools (Read, Glob, Grep). It can read only the batch folder and the downloaded repositories.
- **Limit:** Codex's read-only sandbox can still *read* other files on your computer. The instructions tell the agent to read only the batch, but downloaded code could contain text that tries to trick the agent. For this reason, the client sends only labels, never the agent's full log, and it removes text that looks like a secret. If you want more protection, use Claude Code, or run the client in a separate user account or a virtual machine.

## Install

You need macOS, Linux or Windows, and one of these:

- **Codex:** install it from https://developers.openai.com/codex/cli, then run `codex login`.
- **Claude Code:** install it from https://claude.com/claude-code, then log in.

On macOS or Linux, open the Terminal app and run:

```
curl -fsSL https://raw.githubusercontent.com/yungifez/flows-contribute/main/install.sh | sh
```

The script downloads the client for your computer from the GitHub releases, checks its SHA-256 checksum, and puts it in `~/.local/bin`.

On Windows, download `flows-contribute-x86_64-pc-windows-msvc.exe` from the releases page and run it from a terminal.

## Use

1. Log in with GitHub. Flows asks for no GitHub permissions. It only checks that the account is real and not brand new.

   ```
   flows-contribute login
   ```

2. Start labeling:

   ```
   flows-contribute run
   ```

   Before the first upload, the client shows the terms and the license for your labels. Nothing is sent until you type `yes`.

   Options:

   - `--agent codex` or `--agent claude` selects the agent. The default is Codex if it is installed.
   - `--model <model>` selects a model from the list that Flows accepts.
   - `--once` labels one batch, then stops.

3. See your status: `flows-contribute status`.

## Stop and continue

- Press Ctrl+C at any time. Run `flows-contribute run` again to continue. The server gives you the same batch again until it expires.
- If your usage limit runs out, the client tells you when to try again. Nothing is lost.
- If the agent finished a batch but the upload failed, the next run sends the saved answer. It does not label the batch again.
- Each batch can take 10 to 30 minutes. Keep the computer awake.

## What happens to your labels

Your labels go on a waitlist. A model from a different company labels the same code without seeing your labels. A label is accepted only when two labelers from different companies agree. Some locations in each batch have known answers. If your labels often disagree with them, your account is paused.

You can delete `~/FlowsWork` at any time to free disk space. To remove your login from this computer, run `flows-contribute logout`.

## For maintainers

Build-time settings:

- `FLOWS_SERVER_URL`: the work queue address.
- `FLOWS_GITHUB_CLIENT_ID`: the client id of the GitHub OAuth App, with device flow turned on.

Runtime overrides, mainly for tests: `FLOWS_SERVER`, `FLOWS_GITHUB_CLIENT_ID`, `FLOWS_GITHUB_URL`, `FLOWS_CODELOAD_URL`, `FLOWS_CONFIG` and `FLOWS_WORK`.

The client code is under the MIT license. The license for contributed labels is set by the work queue and shown before the first upload.
