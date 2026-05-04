import tailwindcss from "@tailwindcss/vite";

export default defineNuxtConfig({
  compatibilityDate: "2025-07-15",
  devtools: { enabled: true },
  vite: { plugins: [tailwindcss()] },
  css: ["~/assets/css/main.css"],
  modules: ["@nuxt/content", "@nuxt/eslint", "@nuxt/fonts", "@nuxt/image"],
  routeRules: {
    "/": { redirect: { to: "/docs/getting-started", statusCode: 302 } },
    "/docs": { redirect: { to: "/docs/getting-started", statusCode: 302 } },
    "/docs/**": { prerender: true },
  },
  app: {
    head: {
      link: [{ rel: "icon", type: "image/x-icon", href: "/favico.ico" }],
    },
  },
  content: {
    experimental: { sqliteConnector: "native" },
    renderer: { anchorLinks: false },
    build: {
      markdown: {
        highlight: {
          langs: ["rust", "shell", "typescript"],
          theme: "vitesse-dark",
        },
      },
    },
  },
});
