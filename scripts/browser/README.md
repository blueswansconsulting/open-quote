# Browser regression checks (optional)

Phone-sized (iPhone 13) end-to-end checks with Playwright, driving your installed Chrome. They aren't part of `scripts/ci.sh`, because they need Playwright.

```sh
npm install --no-save playwright          # once
cargo run --release &                     # http://127.0.0.1:8790
node scripts/browser/legacy-users.cjs http://127.0.0.1:8790
node scripts/browser/countries.cjs   http://127.0.0.1:8790 /tmp/oq-pdfs
```

- **`legacy-users.cjs`:** people who used Open Quote before country settings existed always stay on UK settings with their old VAT choice, whatever their browser language. Brand-new visitors get country detection. A link's tax settings survive a reload, and "New quote" resets them.
- **`countries.cjs`:** UK, Australia (GST-inclusive), Canada via a link (GST + PST, Letter), US (labour untaxed), New Zealand (unregistered) and Ireland via a link. It checks the on-screen totals and saves each PDF.

Both print a JSON line per scenario and any page errors. Compare the totals with the README examples.
