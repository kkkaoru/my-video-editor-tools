import { defineConfig, bindings, triggers } from "cf/config";

const CLIPS_QUEUE = "transcript-viewer-clips";

export default defineConfig({
  worker: {
    name: "transcript-viewer-web",
    entrypoint: "src/worker.js",
    compatibilityDate: "2025-06-01",
    workersDev: true,
    domains: process.env.VIEWER_DOMAIN ? process.env.VIEWER_DOMAIN.split(",") : [],
    observability: { enabled: true },
    assets: { runWorkerFirst: true },
    // R2 event notifications for data/*.json keep each video's movie-key pointer current. The Worker
    // has no AI binding: clips are computed on the Mac.
    triggers: [triggers.queue({ name: CLIPS_QUEUE, maxBatchSize: 1, maxConcurrency: 1, maxRetries: 5, retryDelay: 120 })],
    env: {
      ASSETS: bindings.assets(),
      ALLOW_KV: bindings.kv(),
      MEDIA: bindings.r2({ name: "transcript-viewer-media" }),
    },
  },
});
