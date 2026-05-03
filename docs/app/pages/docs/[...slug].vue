<script lang="ts" setup>
definePageMeta({
  layout: "default",
});

const route = useRoute();
const pageId = computed(() => `/docs/${route.path}`);

const { data } = await useAsyncData(pageId, () => {
  return queryCollection("docs").path(route.path).first();
});

const { data: docs } = await useAsyncData(() => {
  return queryCollection("docs").all();
});
</script>

<template>
  <div class="w-full h-full">
    <div
      class="w-full h-full grid grid-cols-[128px_1fr] md:grid-cols-[192px_1fr] sm:gap-6 md:gap-16"
    >
      <div class="flex flex-col gap-3">
        <p
          :class="doc.path == route.path ? '' : 'opacity-60'"
          v-for="doc in docs"
          :key="doc.id"
        >
          <nuxt-link :to="doc.path">
            {{ doc.title }}
          </nuxt-link>
        </p>
      </div>
      <div class="w-full h-full flex flex-col items-center overflow-y-auto">
        <div class="h-full w-full max-w-200">
          <div class="w-full">
            <div v-if="data">
              <article class="prose">
                <h1>{{ data.title }}</h1>
                <ContentRenderer :value="data"> </ContentRenderer>
              </article>
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
