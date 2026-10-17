// Nidavellir error-report relay (Cloudflare Worker, Free plan).
//
// The app never holds a GitHub token. It POSTs an anonymous report here, and this Worker files it as
// an issue in a private repository with a token kept as a Worker secret (fine-grained, that
// repository only, Issues read and write). Reports with the same signature (kind, class, GPU
// family, driver branch, phase, message shape) land on one issue as comments, so fixes target
// classes of errors, not single reports.
//
// Secrets (set with `wrangler secret put`, never in this file): GITHUB_TOKEN, and REPO
// ("owner/name" of the private repository). Optional binding: LIMITER (wrangler.toml).

const MAX_BODY_BYTES = 200_000;
// GitHub rejects issue or comment bodies over 65,536 characters.
const MAX_ISSUE_CHARS = 60_000;
const KINDS = new Set(["error", "summary"]);
const NAME = /^[a-z0-9_]{1,40}$/;

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    if (request.method !== "POST" || url.pathname !== "/report") {
      return new Response("Not found", { status: 404 });
    }
    if (env.LIMITER) {
      const { success } = await env.LIMITER.limit({ key: request.headers.get("cf-connecting-ip") ?? "unknown" });
      if (!success) return new Response("Too many reports", { status: 429 });
    }
    const text = await request.text();
    if (text.length > MAX_BODY_BYTES) return new Response("Report too large", { status: 413 });
    let report;
    try {
      report = JSON.parse(text);
    } catch {
      return new Response("Invalid JSON", { status: 400 });
    }
    const problem = invalid(report);
    if (problem) return new Response(problem, { status: 400 });

    const signature = await signatureOf(report);
    const existing = await github(env, `/repos/${env.REPO}/issues?labels=report,sig:${signature}&state=open&per_page=1`);
    if (!existing.ok) return new Response("Upstream error", { status: 502 });
    const [issue] = await existing.json();
    const body = render(report, signature);
    const filed = issue
      ? await github(env, `/repos/${env.REPO}/issues/${issue.number}/comments`, { body })
      : await github(env, `/repos/${env.REPO}/issues`, {
          title: titleOf(report),
          body,
          labels: ["report", `kind:${report.kind}`, `class:${report.class}`, `gpu:${slug(report.gpu_family)}`, `sig:${signature}`],
        });
    return filed.ok ? new Response("ok") : new Response("Upstream error", { status: 502 });
  },
};

/** A short reason when the report does not have the expected shape; null when it does. */
function invalid(report) {
  if (!report || typeof report !== "object" || Array.isArray(report)) return "Report must be an object";
  if (report.schema !== 1) return "Unknown schema";
  if (!KINDS.has(report.kind)) return "Unknown kind";
  if (!NAME.test(report.class ?? "")) return "Invalid class";
  for (const key of ["install_id", "app_version", "gpu", "gpu_family", "driver"]) {
    if (typeof report[key] !== "string" || report[key].length === 0 || report[key].length > 200) return `Invalid ${key}`;
  }
  return null;
}

/** One issue per error class: numbers are blanked so "1830 MHz" and "1815 MHz" match. */
async function signatureOf(report) {
  const shape = String(report.message ?? "").replace(/\d+/g, "#").replace(/\s+/g, " ").trim().slice(0, 200);
  const driverBranch = String(report.driver).split(".")[0];
  const key = [report.kind, report.class, report.gpu_family, driverBranch, report.forge?.phase ?? "", shape].join("|");
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(key));
  return [...new Uint8Array(digest)].slice(0, 6).map((byte) => byte.toString(16).padStart(2, "0")).join("");
}

function titleOf(report) {
  const what = report.kind === "summary" ? "Forge finished" : report.class.replaceAll("_", " ");
  return `[${report.gpu_family}] ${what}: ${String(report.message ?? "").slice(0, 80)}`.slice(0, 200);
}

function slug(value) {
  return String(value).toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "").slice(0, 40) || "unknown";
}

/** Untrusted text goes inside fences that it cannot close. */
function fenced(label, value) {
  if (value == null || value === "") return "";
  const text = typeof value === "string" ? value : JSON.stringify(value, null, 2);
  return `<details><summary>${label}</summary>\n\n~~~~~~~~\n${text.replaceAll("~~~~~~~~", "~ ~ ~ ~")}\n~~~~~~~~\n\n</details>\n\n`;
}

function render(report, signature) {
  const forge = report.forge ?? {};
  const facts = [
    ["Kind / class", `${report.kind} / ${report.class}`],
    ["App", `${report.app_version} (${report.build ?? "?"})`],
    ["Windows", report.windows ?? "?"],
    ["GPU", `${report.gpu} · ${report.vram_mb ?? "?"} MB VRAM`],
    ["Driver", report.driver],
    ["Forge", `${forge.mode ?? "?"} · ${forge.learning ?? "?"} · phase ${forge.phase ?? "?"} · task ${forge.task ?? "?"}`],
    ["Install", report.install_id.slice(0, 8)],
    ["Signature", signature],
  ];
  const table = ["| | |", "|---|---|", ...facts.map(([name, value]) => `| ${name} | ${String(value).replaceAll("|", "\\|")} |`)].join("\n");
  let body = `${table}\n\n**${String(report.message ?? "").slice(0, 500)}**\n\n`;
  body += fenced("Panic", report.panic);
  body += fenced("Live log events", report.events);
  body += fenced("Forge result", report.result);
  body += fenced("Forge log", report.forge_log);
  body += fenced("Driver events", report.driver_events);
  body += fenced("Core log (tail)", report.core_log);
  return body.length > MAX_ISSUE_CHARS ? `${body.slice(0, MAX_ISSUE_CHARS)}\n\n…(truncated)` : body;
}

function github(env, path, payload) {
  return fetch(`https://api.github.com${path}`, {
    method: payload ? "POST" : "GET",
    headers: {
      Accept: "application/vnd.github+json",
      Authorization: `Bearer ${env.GITHUB_TOKEN}`,
      "User-Agent": "nidavellir-report-relay",
      "X-GitHub-Api-Version": "2022-11-28",
      ...(payload ? { "Content-Type": "application/json" } : {}),
    },
    body: payload ? JSON.stringify(payload) : undefined,
  });
}
