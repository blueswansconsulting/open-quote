# Changelog

All notable changes to this project are documented here. The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html). Until 1.0.0, the HTTP API and the JSON schema may change between minor versions.

## [Unreleased]

## [0.1.0] - 2026-11-20

### Added
- Example roofing and solar quotes in `examples/`.
- Local CI script (`scripts/ci.sh`) and pre-push hook.
- A mobile quote form: business details, customer, job description, line items with labour and materials tags, a 20% VAT toggle, photos and terms.
- A dependency-free PDF writer: A4, multi-page, logo, up to 3 photos, a clickable trust footer with rating, and "Page X of Y".
- Sending through the phone's share sheet, with a fallback that downloads the PDF and offers WhatsApp and email buttons.
- Draft saved on the device, a "New quote" button and automatic quote numbering.
- An anonymous PDF counter (`/api/stats`), `/healthz` and the `open-quote healthcheck` command.
- Security headers (CSP, nosniff, frame-deny), a 6 MB body limit and input validation.
- Prefill from a link (`#customer_name=…&item=…`) for CRMs and enquiry emails; customer fields are stripped from the address bar and UTM tags are kept.
- Quote numbers follow your own format (for example `SE-0042`), and editing a sent quote moves it to a new number.
- A Docker image, Compose file and CI.

[Unreleased]: https://github.com/blueswansconsulting/open-quote/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/blueswansconsulting/open-quote/releases/tag/v0.1.0
