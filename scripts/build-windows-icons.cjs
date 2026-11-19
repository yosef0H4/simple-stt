// Regenerate the checked-in Windows icons with Node and Playwright.
const fs = require("node:fs/promises");
const path = require("node:path");
const { chromium } = require("playwright");

const root = path.resolve(__dirname, "..");
const icons = path.join(root, "ahk", "icons");
const revision = "99701e07d3d1fe1bdbcdb14964547f0fd592966f";
const noto = `https://raw.githubusercontent.com/googlefonts/noto-emoji/${revision}`;
const emoji = {
  speaker: "1f50a", camera: "1f4f7", clipboard: "1f4cb", book: "1f4d6",
  stop: "1f6d1", settings: "2699", unload: "1f4e4", keyboard: "2328",
  reload: "1f504", log: "1f4c4", test: "1f9ea", exit: "1f6aa",
};

async function download(relative) {
  const response = await fetch(`${noto}/${relative}`);
  if (!response.ok) throw new Error(`${response.status} reading Noto Emoji ${relative}`);
  return Buffer.from(await response.arrayBuffer());
}

async function renderIco(page, filename, markup, sizes) {
  await page.setContent(`<style>
    body { margin: 0; }
    svg, img { display: block; width: 100vw; height: 100vh; object-fit: contain; }
  </style>${markup}`);
  const table = Buffer.alloc(6 + sizes.length * 16);
  table.writeUInt16LE(1, 2);
  table.writeUInt16LE(sizes.length, 4);
  const images = [];
  let offset = table.length;
  for (const [index, size] of sizes.entries()) {
    await page.setViewportSize({ width: size, height: size });
    const pixels = await page.screenshot({ omitBackground: true });
    const entry = 6 + index * 16;
    table[entry] = table[entry + 1] = size % 256;
    table.writeUInt16LE(1, entry + 4);
    table.writeUInt16LE(32, entry + 6);
    table.writeUInt32LE(pixels.length, entry + 8);
    table.writeUInt32LE(offset, entry + 12);
    offset += pixels.length;
    images.push(pixels);
  }
  await fs.writeFile(path.join(icons, filename + ".ico"), Buffer.concat([table, ...images]));
}

async function main() {
  await fs.mkdir(icons, { recursive: true });
  const sources = await Promise.all(Object.entries(emoji).map(async ([name, code]) => [
    name, await download(`2D/png/128/emoji_u${code}.png`),
  ]));
  const notice = await download("2D/svg/LICENSE");
  const license = await fetch("https://www.apache.org/licenses/LICENSE-2.0.txt");
  if (!license.ok) throw new Error("Could not read the Apache License 2.0");
  const logo = await fs.readFile(path.join(root, "packaging/linux/simple-stt.svg"), "utf8");
  const browser = await chromium.launch();
  try {
    const page = await browser.newPage();
    await renderIco(page, "simple-stt", logo, [16, 20, 24, 32, 48, 128, 256]);
    for (const [name, png] of sources) {
      await renderIco(page, name, `<img src="data:image/png;base64,${png.toString("base64")}">`, [16, 20, 24, 32]);
    }
    await fs.writeFile(path.join(icons, "LICENSE-NOTO-EMOJI.txt"),
      Buffer.concat([notice, Buffer.from("\n"), Buffer.from(await license.arrayBuffer())]));
  } finally {
    await browser.close();
  }
}

main().catch(error => {
  console.error(error.message);
  process.exitCode = 1;
});
