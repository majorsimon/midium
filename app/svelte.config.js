import adapter from "@sveltejs/adapter-static";
import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";

/** @type {import('@sveltejs/kit').Config} */
const config = {
  preprocess: vitePreprocess(),
  kit: {
    adapter: adapter({
      pages: "dist",
      assets: "dist",
      fallback: "index.html",
      precompress: false,
    }),
    prerender: {
      handleHttpError: ({ path, referrer, message }) => {
        // Ignore missing favicon — Tauri provides its own
        if (path === "/favicon.png") return;
        throw new Error(message);
      },
    },
  },
};

export default config;
