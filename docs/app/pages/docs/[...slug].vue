<script lang="ts" setup>
definePageMeta({ layout: "docs" });

const route = useRoute();
const pageId = computed(() => `/docs/${route.path}`);
const { data } = await useAsyncData(pageId, () => {
  return queryCollection("docs").path(route.path).first();
});
</script>

<template>
  <div>
    <div v-if="data">
      <h1>{{ data.title }}</h1>
      <article>
        <ContentRenderer class="prose-xl" :value="data"> </ContentRenderer>
      </article>
    </div>
  </div>
</template>
