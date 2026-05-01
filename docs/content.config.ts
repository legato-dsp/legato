import {
  defineContentConfig,
  defineCollection,
  z,
  defineCollectionSource,
} from "@nuxt/content";
import nodesData from "./content/nodes.json";

const nodesSource = defineCollectionSource({
  getKeys: async () => nodesData.map((x) => `${x.name}.json`),
  getItem: async (key: string) => {
    const name = key.replace(/\.json$/, "");
    const node = nodesData.find((n) => n.name === name)!;
    return JSON.stringify(node);
  },
});

export default defineContentConfig({
  collections: {
    docs: defineCollection({
      type: "page",
      source: "docs/*.md",
      schema: z.object({
        title: z.string(),
      }),
    }),
    nodes: defineCollection({
      type: "data",
      source: nodesSource,
      schema: z.object({
        name: z.string(),
        description: z.string(),
        required_params: z.array(z.string()),
        optional_params: z.array(z.string()),
      }),
    }),
  },
});
