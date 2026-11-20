# Contributing

Thanks for helping. Open Quote is deliberately small: one job, done well, for one-person trade businesses.

## Before you start

- **Bugs:** open an issue using the bug form. Include the JSON you sent (with personal details removed) or the steps you took.
- **Features:** open a feature request first. We keep the scope tight. Invoicing, payments, job management and certificates are out of scope.
- **Security issues:** don't open an issue. See [SECURITY.md](SECURITY.md).

## Development

```sh
scripts/ci.sh                        # the full check suite; must pass before a PR
git config core.hooksPath .githooks  # run it automatically on git push
cargo run                            # http://127.0.0.1:8790
```

- Money is integer pence. Never introduce floating-point totals.
- The server must stay stateless: don't add storage or logging of quote content.
- Add a test for every bug fix and for any new validation rule.
- Keep the page free of external scripts, fonts and trackers.

## Pull requests

- Keep each PR focused on one change, and fill in the PR template.
- `scripts/ci.sh` must pass. Paste its final lines into the PR.
- By submitting a PR you agree your contribution is licensed under the project's [AGPL-3.0](LICENSE) licence. Sign off your commits (`git commit -s`) to confirm the [Developer Certificate of Origin](https://developercertificate.org/).
