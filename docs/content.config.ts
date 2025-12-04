import { defineContentConfig, defineCollection } from "@nuxt/content";
import { z } from "zod";

export default defineContentConfig({
  collections: {
    docs: defineCollection({
      type: "page",
      source: "docs/*.md",
      schema: z.object({
        title: z.string(),
      }),
    }),
  },
});
