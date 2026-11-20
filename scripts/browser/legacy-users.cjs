const { chromium, devices } = require("playwright");
(async () => {
  const base = process.argv[2];
  const browser = await chromium.launch({ channel: "chrome" });
  const errors = [];
  const results = [];
  async function scenario(name, locale, seed, url = "/") {
    const ctx = await browser.newContext({ ...devices["iPhone 13"], locale });
    const page = await ctx.newPage();
    page.on("pageerror", (e) => errors.push(`${name}: ${e.message}`));
    await page.goto(base + "/healthz");
    await page.evaluate((seed) => { localStorage.clear(); for (const [k, v] of Object.entries(seed)) localStorage.setItem(k, v); }, seed);
    await page.goto(base + url, { waitUntil: "networkidle" });
    const read = () => page.evaluate(() => ({
      summary: document.getElementById("tax_summary").textContent,
      vatChecked: document.getElementById("vat").checked,
      totals: [...document.querySelectorAll("#totals > div")].map((d) => d.innerText.replace(/\s+/g, " ").trim()).join(" | "),
      banner: !document.getElementById("link_banner").hidden,
      savedTax: localStorage.getItem("qm_tax"),
    }));
    let r = await read();
    if (name === "link-reload") { await page.reload({ waitUntil: "networkidle" }); r.afterReload = await read();
      await page.click("#new_quote"); await page.click("#new_quote"); r.afterNewQuote = await read(); }
    results.push([name, r]);
    await ctx.close();
  }
  const draft = JSON.stringify({ customer: { name: "Mrs Jones", address: "" }, scope: "", items: [{ description: "Socket", quantity: "1", unit_price: "100", kind: "" }], quote_number: "Q-0003", date: "2026-11-20", days: "30" });
  for (const locale of ["en-GB", "en-US", "en-AU"]) {
    for (const vat of [null, "0", "1"]) {
      const seed = { qm_b_name: "Old UK Trade", qm_draft: draft };
      if (vat !== null) seed.qm_vat = vat;
      await scenario(`legacy ${locale} qm_vat=${vat}`, locale, seed);
    }
  }
  await scenario("new visitor en-AU", "en-AU", {});
  await scenario("new visitor en-GB", "en-GB", {});
  await scenario("link-reload", "en-GB", { qm_b_name: "Old UK Trade" }, "/#country=AU&customer_name=Bob&item=Job%7C1%7C1100");
  for (const [n, r] of results) console.log(n.padEnd(28), JSON.stringify(r));
  console.log("page errors:", JSON.stringify(errors));
  await browser.close();
})().catch((e) => { console.error(e); process.exit(1); });
