# Code signing policy

Windows builds of zunder (`zunder.exe`, `zunder-console.exe` and `zunder-windows-setup.exe`)
are signed.

Free code signing provided by [SignPath.io](https://about.signpath.io), certificate by [SignPath Foundation](https://signpath.org)

## How a release gets signed

- Every Windows file is built from this repository by the
  [release workflow](../.github/workflows/release.yml) on GitHub-hosted runners, never on a
  personal machine.
- The workflow sends the unsigned exes to SignPath. An approver checks the request (the tag,
  the commit and the workflow run it came from) and approves it by hand. Only then are the
  files signed and put on the [release page](https://github.com/elyesghazel/zunder/releases).
- Only zunder's own programs are signed. Bundled third-party files (GTK DLLs, `fd.exe`) keep
  the signature their authors gave them, or none.

## Team

- Committers and reviewers: [elyesghazel](https://github.com/elyesghazel)
- Approvers: [elyesghazel](https://github.com/elyesghazel)

Changes from anyone else come in as pull requests and are reviewed before they're merged.
Everyone on the team uses multi-factor authentication for GitHub and SignPath.

## Privacy

This program will not transfer any information to other networked systems unless
specifically requested by the user or the person installing or operating it, with one
exception that is on by default and can be turned off:

- **Update check:** once a day zunder asks GitHub (`api.github.com`) for the newest release
  and shows a note on the start screen. Off with Settings → General → Check for updates, or
  `update_check = false` in `config.toml`.

Everything else only happens when you ask for it:

- **Plugins:** Settings → Plugins and `zunder plugin` download the plugin list and plugins
  from GitHub (`raw.githubusercontent.com`).
- **Currency conversion:** typing a conversion (`20 usd to chf`) loads exchange rates from
  `open.er-api.com`, at most every 12 hours.
- **Login and sync:** after `zunder login` (or Settings → Account), zunder syncs your settings
  and favorites to a private gist on your own GitHub account. Plugin tokens are never synced.
- **Web search and links** open in your browser.

zunder has no analytics, no telemetry and no account of its own. Plugins are separate programs
you install yourself and can do their own networking; each one's README says what it talks to.
The installer shows this list before it installs anything.

## Report a problem

If a signed zunder file does something this page doesn't describe, open an
[issue](https://github.com/elyesghazel/zunder/issues).
