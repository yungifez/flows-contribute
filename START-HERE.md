# Start here: help Flows with your Codex plan

This guide is for people who do not write code. It takes about 10 minutes to set up. After that, your computer does the work.

**What you give:** some of the Codex usage that your ChatGPT plan includes.
**What it does:** Codex reads public open-source code and labels what each part does. Flows uses the labels to teach AI coding tools about common app features.

## Option A: let Codex set it up for you

1. Open Codex (the app, or `codex` in the Terminal).
2. Copy the message below and paste it into Codex:

   > Please set up the Flows contributor client for me. Read https://github.com/yungifez/flows-contribute/blob/main/START-HERE.md and do Option B steps 1 and 2 for me: check Codex, install the client, and fix my PATH if needed. Do not run `flows-contribute login` or `flows-contribute run`. When you finish, tell me in simple words what you did.

3. Then do Option B steps 3 and 4 yourself in the Terminal. The login shows a code that you must see, and one batch can take 30 minutes, so these steps work better outside Codex.

## Option B: do it yourself

### Step 1: check that you have Codex

Open the **Terminal** app (on a Mac: press Cmd+Space, type "Terminal", press Enter). Type this and press Enter:

```
codex login status
```

- If it says you are logged in, go to step 2.
- If it says `command not found`, install Codex first: https://developers.openai.com/codex/cli. Then run `codex login` and log in with your ChatGPT account.

### Step 2: install the Flows client

Paste this into the Terminal and press Enter:

```
curl -fsSL https://raw.githubusercontent.com/yungifez/flows-contribute/main/install.sh | sh
```

If the last lines say "Add … to your PATH", paste this too, so that the Terminal can find the client:

```
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zshrc && source ~/.zshrc
```

(On Linux, use `~/.bashrc` instead of `~/.zshrc`.)

On **Windows**, download `flows-contribute-x86_64-pc-windows-msvc.exe` from https://github.com/yungifez/flows-contribute/releases/latest. In the commands below, use the path of that file instead of `flows-contribute`.

### Step 3: log in with GitHub

You need a GitHub account that is at least 90 days old. Flows asks for no GitHub permissions.

```
flows-contribute login
```

It shows a code like `ABCD-1234`. Open https://github.com/login/device, enter the code and approve. The Terminal then says "You are logged in".

### Step 4: start labeling

```
flows-contribute run
```

- The first time, it shows the terms and the license for your labels. Read them and type `yes` to agree.
- It downloads some code (up to about 1 GB in total), then Codex starts. Each batch takes 10 to 30 minutes.
- Leave the Terminal open and keep the computer awake. On a Mac, you can run `caffeinate -i flows-contribute run` instead, so that the Mac does not sleep.
- It continues batch after batch until your Codex limit or the daily limit (8 batches). Then it stops by itself and tells you.

## Messages you can see

| Message | What to do |
| --- | --- |
| "Uploads are not open yet" | Flows is not ready yet. Try again later. |
| "Your Codex usage limit is used up" | Nothing is lost. Run `flows-contribute run` again after the time it shows. |
| "You reached today's limit" | Thank you! Run it again tomorrow. |
| "There is no work right now" | Thank you! Try again later. |
| "You are not logged in" | Do step 3 again. |
| Any other problem | Run the same command again. If it fails again, send a screenshot to the person who invited you. |

To stop at any time, press **Ctrl+C**. To continue, run `flows-contribute run` again.

## Is it safe?

- The downloaded code never runs. Codex only reads it, in read-only mode.
- Flows receives only the labels and token counts, not your files and not Codex's full logs.
- One limit: Codex in read-only mode can still read other files on your computer. Flows tells it to read only the downloaded code, and the client removes anything that looks like a password or key before upload. For more detail, see [README.md](README.md#safety).

To check how many labels you sent: `flows-contribute status`.
To free disk space afterwards: delete the `FlowsWork` folder in your home folder.
