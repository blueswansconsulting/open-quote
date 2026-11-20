const { chromium, devices } = require("playwright");
(async () => {
  const [base, out] = process.argv.slice(2);
  const browser = await chromium.launch({ channel: "chrome" });
  const errors = [];
  async function run(name, { locale = "en-GB", url = "/", setup } = {}) {
    const ctx = await browser.newContext({ ...devices["iPhone 13"], locale, acceptDownloads: true });
    const page = await ctx.newPage();
    page.on("pageerror", (e) => errors.push(`${name}: ${e.message}`));
    await page.goto(base + url, { waitUntil: "networkidle" });
    if (setup) await setup(page);
    if (!(await page.inputValue("#b_name"))) await page.fill("#b_name", "Test Trade");
    if (!(await page.inputValue("#c_name"))) await page.fill("#c_name", "Customer");
    const rows = page.locator("#items .item");
    if (!(await rows.first().locator(".desc").inputValue())) {
      await rows.first().locator(".desc").fill("Materials"); await rows.first().locator(".price").fill("500");
      await rows.first().locator(".kind").selectOption("materials");
      await page.click("#add");
      await rows.nth(1).locator(".desc").fill("Labour"); await rows.nth(1).locator(".price").fill("300");
      await rows.nth(1).locator(".kind").selectOption("labour");
    }
    const info = await page.evaluate(() => ({
      summary: document.getElementById("tax_summary").textContent,
      cardOpen: document.getElementById("taxcard").open,
      toggle: document.getElementById("vat_wrap").hidden ? "(hidden)" : document.getElementById("vat_label").textContent,
      totals: [...document.querySelectorAll("#totals > div")].map((d) => d.innerText.replace(/\s+/g, " ").trim()),
      banner: document.getElementById("link_banner").hidden ? "" : document.getElementById("link_banner_text").textContent,
      savedTax: localStorage.getItem("qm_tax") ? JSON.parse(localStorage.getItem("qm_tax")).country : null,
      taxLinks: [...document.querySelectorAll(".tax-link")].map((b) => b.hidden ? "-" : b.textContent),
    }));
    const [dl] = await Promise.all([page.waitForEvent("download"), page.click("#download")]);
    await dl.saveAs(`${out}/${name}.pdf`);
    console.log(name, JSON.stringify(info));
    await ctx.close();
  }
  await run("uk");
  await run("au", { locale: "en-AU" });
  await run("ca-bc-link", { url: "/?utm_source=crm#country=CA&province=BC" });
  await run("us", { locale: "en-US", setup: async (p) => { await p.fill("#t1_rate", "8.875"); await p.locator("#t1_rate").blur(); await p.waitForTimeout(100); } });
  await run("nz-unregistered", { locale: "en-NZ", setup: async (p) => { await p.check("#t_reg_no"); } });
  await run("ie-link-rate", { url: "/#country=IE&tax_rate=13.5&title=quote&paper=letter" });
  console.log("page errors:", JSON.stringify(errors));
  await browser.close();
})().catch((e) => { console.error(e); process.exit(1); });
