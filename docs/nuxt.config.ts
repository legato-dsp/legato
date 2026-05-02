import tailwindcss from "@tailwindcss/vite";

export default defineNuxtConfig({
  compatibilityDate: "2025-07-15",
  devtools: { enabled: true },
  vite: { plugins: [tailwindcss()] },
  css: ["~/assets/css/main.css"],
  modules: ["@nuxt/content", "@nuxt/eslint", "@nuxt/fonts", "@nuxt/image"],
  content: {
    experimental: { sqliteConnector: "native" }, // Required for vercel
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
