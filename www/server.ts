import { resolve, join } from "path";

const PORT = Number(process.env.PORT) || 3000;
const openPlayground = process.argv.includes("playground");
const ROOT = resolve(import.meta.dir, "..");
const WWW_DIR = resolve(import.meta.dir);

const MIME: Record<string, string> = {
  ".html": "text/html",
  ".css": "text/css",
  ".js": "application/javascript",
  ".ts": "application/javascript",
  ".json": "application/json",
  ".png": "image/png",
  ".svg": "image/svg+xml",
  ".ico": "image/x-icon",
};

const buildCache = new Map<string, string>();

async function buildPlayground(): Promise<string> {
  if (buildCache.has("playground")) return buildCache.get("playground")!;

  const entrypoint = join(WWW_DIR, "playground.ts");
  const result = await Bun.build({
    entrypoints: [entrypoint],
    outdir: "/tmp/opencode-build",
    target: "browser",
    format: "esm",
    naming: "[name].[ext]",
    plugins: [
      {
        name: "path-alias",
        setup(build) {
          build.onResolve({ filter: /^@src\// }, (args) => {
            return { path: join(ROOT, args.path.replace("@src/", "src/")) };
          });
          build.onResolve({ filter: /^@www\// }, (args) => {
            return { path: join(ROOT, args.path.replace("@www/", "www/")) };
          });
          build.onResolve({ filter: /^@package\.json$/ }, () => {
            return { path: join(ROOT, "package.json") };
          });
        },
      },
    ],
  });

  if (result.success && result.outputs.length > 0) {
    const js = await result.outputs[0].text();
    buildCache.set("playground", js);
    return js;
  }

  throw new Error("Build failed: " + result.logs.map(l => ("text" in l ? l.text : "")).join("\n"));
}

Bun.serve({
  port: PORT,
  async fetch(req) {
    const url = new URL(req.url);
    let pathname = url.pathname === "/" ? "/index.html" : url.pathname;

    if (openPlayground && pathname === "/") pathname = "/playground.html";

    if (pathname === "/playground.ts") {
      try {
        const js = await buildPlayground();
        return new Response(js, {
          headers: { "Content-Type": "application/javascript" },
        });
      } catch (e) {
        return new Response("Build error: " + (e as Error).message, {
          status: 500,
          headers: { "Content-Type": "text/plain" },
        });
      }
    }

    const filePath = join(WWW_DIR, pathname);
    const srcPath = join(ROOT, pathname);

    let file = Bun.file(filePath);
    if (!(await file.exists())) {
      file = Bun.file(srcPath);
    }

    if (await file.exists()) {
      const ext = pathname.substring(pathname.lastIndexOf("."));
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
