const PORT = Number(process.env.PORT) || 3000;
const openPlayground = process.argv.includes("playground");

const MIME: Record<string, string> = {
  ".html": "text/html",
  ".css": "text/css",
  ".js": "application/javascript",
  ".json": "application/json",
  ".png": "image/png",
  ".svg": "image/svg+xml",
  ".ico": "image/x-icon",
};

Bun.serve({
  port: PORT,
  async fetch(req) {
    const url = new URL(req.url);
    let path = url.pathname === "/" ? "/index.html" : url.pathname;

    if (openPlayground && path === "/") path = "/playground.html";

    const file = Bun.file(`./www${path}`);
    if (await file.exists()) {
      const ext = path.substring(path.lastIndexOf("."));
      return new Response(file, {
        headers: { "Content-Type": MIME[ext] || "text/plain" },
      });
    }

    return new Response("Not Found", { status: 404 });
  },
});

console.log(`\n  Workflow Playground running at:\n`);
console.log(`    http://localhost:${PORT}/playground.html`);
console.log(`\n  Press Ctrl+C to stop.\n`);
