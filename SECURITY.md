# Security policy

## Supported versions

Open Quote is pre-1.0. Only the latest release on `main` receives security fixes.

## Reporting a vulnerability

**Please don't open a public issue.** Report it privately through GitHub:

**[Report a vulnerability](https://github.com/blueswansconsulting/open-quote/security/advisories/new)** (Security tab → "Report a vulnerability").

Include what you found, how to reproduce it, and the impact you expect. What happens next:

- **Within 5 working days:** we acknowledge your report.
- **Within 14 days:** we assess it and agree a fix plan with you.
- **Once a fix is released:** we publish a GitHub security advisory and credit you, unless you'd rather stay anonymous.

## Scope notes

- The server is stateless. It keeps no quote content, logs only an anonymous PDF count, and has no accounts.
- **In scope:**
  - anything that lets one user read another user's quote data
  - code execution
  - PDF or HTTP header injection
  - crashes from crafted input
  - XSS in the form page
- **Out of scope:**
  - Denial of service through large volumes of valid requests. Run behind a reverse proxy with rate limits; see the README.
